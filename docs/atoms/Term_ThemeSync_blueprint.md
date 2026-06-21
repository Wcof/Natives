# Blueprint: Term_ThemeSync (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
终端配色与app皮肤联动，retheme方法动态切换xterm主题，JetBrains Mono/SF Mono等宽字+舒适行距+光标与选区配色随皮肤。

### Reference Code Analysis
- **Target File:** `src-tauri/src/terminal/themesync.rs`
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
Term_ThemeSync:  enabled → disabling → disabled → enabling → enabled
```
- `enabled`: 启用状态
- `disabling`: 正在停用（移入 _disabled/）
- `disabled`: 已停用
- `enabling`: 正在恢复（移出 _disabled/）

### Algorithm
```
// Term_ThemeSync — 启停切换（move into/out of _disabled/）
async fn toggle(name: &str, enable: bool) -> Result<()> {
    let src = skill_path(name)?;
    let dst = if enable {
        // 从 _disabled/ 移出
        src.parent().unwrap().join(&name)
    } else {
        // 移入 _disabled/
        src.parent().unwrap().join("_disabled").join(&name)
    };
    
    // 跨设备 fallback
    match tokio::fs::rename(&src, &dst).await {
        Ok(_) => Ok(()),
        Err(e) if e.raw_os_error() == Some(18) => { // EXDEV
            // 跨挂载点: copy + delete
            copy_dir(&src, &dst).await?;
            tokio::fs::remove_dir_all(&src).await?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/terminal/themesync.rs` |
| 前端组件 | `src/components/terminal/` |

### Data Model
```typescript
// Term_ThemeSync — 终端数据模型
interface Term_ThemeSyncParams {
  id: string;
  // 特征专属参数
}
interface Term_ThemeSyncResult {
  success: boolean;
  id: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn themesync(params: ...) -> Result<Term_ThemeSyncResult>
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
1. **边界条件**: 极端路径/空结果/权限不足

### Mitigation & Runtime Guardrail
- 输入校验前置，拒绝非法路径/空字节/目录穿越
- 所有异步操作超时控制（`tokio::time::timeout`），防死锁
- 单元测试 + 集成测试覆盖边界条件（空目录 / 权限不足 / 符号链接循环 / 并发写入）
- 日志分级输出，关键路径有 error / warn / info 三级追踪
