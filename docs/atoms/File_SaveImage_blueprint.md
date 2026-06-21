# Blueprint: File_SaveImage (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
图片编辑保存(saveImage)，支持dataUrl写入+newName另存为，覆盖原图加确认弹窗(不可逆+有损警告)，保存失败冒泡错误。

### Reference Code Analysis
- **Target File:** `src-tauri/src/file_manager/saveimage.rs`
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
File_SaveImage:  clean → dirty → saving → (saved | conflict | error) → clean
```
- `clean`: 初始/已保存状态
- `dirty`: 内容已修改
- `saving`: 正在落盘（原子写）
- `saved`: 保存成功
- `conflict`: 外部修改冲突（expectedMtime 不匹配）
- `error`: 保存失败

### Algorithm
```
// File_SaveImage — 原子写 + conflict guard + undo stack
async fn save(path: &str, content: &[u8], expected_mtime: Option<i64>) -> Result<WriteResult> {
    // 0. Conflict guard: 检查 expectedMtime
    if let Some(exp) = expected_mtime {
        let cur = stat_mtime(path).await?;
        if (cur - exp).abs() > 1 { return Err(ConflictError); }
    }
    
    // 1. 原子写: temp + fsync + rename
    let tmp = format!("{}.n2-tmp-{}", path, pid());
    let mut f = tokio::fs::File::create(&tmp).await?;
    f.write_all(content).await?;
    f.sync_all().await?;  // fsync 保证落盘
    tokio::fs::rename(&tmp, path).await?;  // POSIX 原子 rename
    
    // 2. 返回新 mtime for next conflict guard
    Ok(WriteResult { ok: true, mtime: Some(stat_mtime(path).await?) })
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/file_manager/saveimage.rs` |
| 前端组件 | `src/components/files/` |

### Data Model
```typescript
// File_SaveImage — 文件管理器数据模型
interface File_SaveImageParams {
  path: string;
  // 特征专属参数
}
interface File_SaveImageResult {
  success: boolean;
  path: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn saveimage(params: ...) -> Result<File_SaveImageResult>
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
