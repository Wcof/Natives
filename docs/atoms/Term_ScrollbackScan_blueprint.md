# Blueprint: Term_ScrollbackScan (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
终端scrollback回扫路径识别(scanScrollbackFor)，从终端缓冲区提取可点击路径，缓存验证结果。

### Reference Code Analysis
- **Target File:** `src-tauri/src/terminal/scrollbackscan.rs`
- **Referenced Functions:** —
- **Optimized Implementation Logic（Natives Rust 优化版）:**
  基于 fanbox.md Ground Truth 描述，该特征的核心逻辑涉及 `—`。
  Natives 重构为 Rust + Tauri，采用异步非阻塞设计：
  - 所有 I/O 通过 `tokio::fs` 异步执行，不阻塞主线程
  - 状态变更通过 Tauri Event 推送，前端响应式更新
  - 类型安全：Rust enum/struct 编译期消除动态类型错误
  - IPC 安全：Tauri command whitelist，无 HTTP localhost 攻击面

### State Machine
```
Term_ScrollbackScan:  idle → scanning → (parsing | validating) → (scanned | error) → idle
```
- `idle`: 初始
- `scanning`: 递归遍历目录
- `parsing`: 解析 frontmatter / 配置
- `validating`: 健康检查 / 重复检测
- `scanned`: 扫描完成，返回结果
- `error`: 扫描中断或出错

### Algorithm
```
// Term_ScrollbackScan — 递归扫描 + 软链跟随 + 健康检查
async fn scan_roots(roots: &[PathBuf]) -> Result<Vec<ScanItem>> {
    let mut results = Vec::new();
    for root in roots {
        let mut dir = tokio::fs::read_dir(root).await?;
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "_archive" || name == "_backups" { continue; }
            
            let ft = entry.file_type().await?;
            let path = entry.path();
            // 软链跟随
            let is_dir = if ft.is_symlink() {
                fs::metadata(&path)?.is_dir()
            } else { ft.is_dir() };
            if !is_dir { results.push(ScanItem::residue(name, path)); continue; }
            
            // 验证 SKILL.md + frontmatter
            let item = validate_skill_dir(&path).await?;
            results.push(item);
        }
    }
    Ok(results)
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/terminal/scrollbackscan.rs` |
| 前端组件 | `src/components/terminal/` |

### Data Model
```typescript
// Term_ScrollbackScan — 终端数据模型
interface Term_ScrollbackScanParams {
  id: string;
  // 特征专属参数
}
interface Term_ScrollbackScanResult {
  success: boolean;
  id: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn scrollbackscan(params: ...) -> Result<Term_ScrollbackScanResult>
```

### Performance & Security Design
| 维度 | 方案 |
|------|------|
| **异步** | 所有 I/O 通过 tokio async 执行，主线程零阻塞 |
| **内存** | 内存边界控制（大文件截断 / LRU 缓存上限 / 流式处理） |
| **安全** | Tauri IPC whitelist + 路径校验 / sandbox iframe / validName |
| **跨平台** | `#[cfg(target_os)]` 条件编译处理平台差异 |

## 3. Risk Analysis

### Risks
1. **缓存**: 缓存过期/一致性/内存泄漏

### Mitigation & Runtime Guardrail
- 输入校验前置，拒绝非法路径/空字节/目录穿越
- 所有异步操作超时控制（`tokio::time::timeout`），防死锁
- 单元测试 + 集成测试覆盖边界条件（空目录 / 权限不足 / 符号链接循环 / 并发写入）
- 日志分级输出，关键路径有 error / warn / info 三级追踪
