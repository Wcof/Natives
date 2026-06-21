# Blueprint: File_RenameTrash (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
重命名(validName校验+同名检测+失败清理tmp)与废纸篓删除(macOS: AppleScript POSIX file as alias强转防-1728/路径走argv防注入/未授权-1743给中文提示；Windows: VB FileSystem SendToRecycleBin；Linux: gio trash/trash-put/trash)，可恢复。

### Reference Code Analysis
- **Target File:** `src-tauri/src/file_manager/renametrash.rs`
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
File_RenameTrash:  idle → confirming → trashing → (done | error) → idle
```
- `idle`: 初始
- `confirming`: 确认弹窗（不可逆操作提示）
- `trashing`: 移入废纸篓
- `done`: 成功
- `error`: 失败（权限 / 跨设备）

### Algorithm
```
// File_RenameTrash — 废纸篓 + validateDir guard
async fn trash(path: &str) -> Result<TrashResult> {
    // 1. 安全校验
    validate_path_allowed(path)?;
    
    // 2. 平台特定废纸篓
    #[cfg(target_os = "macos")]
    { osascript_trash(path).await?; }
    #[cfg(target_os = "linux")]
    { gio_trash(path).await?; }
    #[cfg(target_os = "windows")]
    { vb_trash(path).await?; }
    
    Ok(TrashResult { path: path.into(), recoverable: true })
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/file_manager/renametrash.rs` |
| 前端组件 | `src/components/files/` |

### Data Model
```typescript
// File_RenameTrash — 文件管理器数据模型
interface File_RenameTrashParams {
  path: string;
  // 特征专属参数
}
interface File_RenameTrashResult {
  success: boolean;
  path: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn renametrash(params: ...) -> Result<File_RenameTrashResult>
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
1. **跨平台**: 操作系统的平台 API 差异

### Mitigation & Runtime Guardrail
- 输入校验前置，拒绝非法路径/空字节/目录穿越
- 所有异步操作超时控制（`tokio::time::timeout`），防死锁
- 单元测试 + 集成测试覆盖边界条件（空目录 / 权限不足 / 符号链接循环 / 并发写入）
- 日志分级输出，关键路径有 error / warn / info 三级追踪
