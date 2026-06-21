# Blueprint: File_SidebarDragWindow (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
按住侧栏空白可拖动窗口(对应issue #6)，侧栏整体不设为窗口拖拽区(否则收不到滚轮事件导致滚动时灵时卡)，拖窗用品牌区和顶栏。

### Reference Code Analysis
- **Target File:** `src-tauri/src/file_manager/sidebardragwindow.rs`
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
File_SidebarDragWindow:  idle → hover → dragging → (drop | cancel) → (saving | copying) → done
```
- `idle`: 初始
- `hover`: 拖拽物悬浮在目标区
- `dragging`: 拖拽中
- `drop` / `cancel`: 释放或取消
- `saving` / `copying`: 落盘 / 复制
- `done`: 完成

### Algorithm
```
// File_SidebarDragWindow — 拖拽落盘（去重 + 文件类型检测）
async fn handle_drop(paths: Vec<DropItem>, dest: &str) -> Result<Vec<String>> {
    let mut saved = Vec::new();
    for item in paths {
        let name = if has_conflict(dest, &item.name) {
            unique_name(dest, &item.name)  // "foo 2.png"
        } else { item.name };
        let target = Path::new(dest).join(&name);
        
        match item.source {
            DropSource::FileSystem(path) => {
                tokio::fs::copy(&path, &target).await?;  // copyInto
            }
            DropSource::TempData(data) => {
                tokio::fs::write(&target, &data).await?;  // saveTemp
            }
        }
        saved.push(name);
    }
    Ok(saved)
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/file_manager/sidebardragwindow.rs` |
| 前端组件 | `src/components/files/` |

### Data Model
```typescript
// File_SidebarDragWindow — 文件管理器数据模型
interface File_SidebarDragWindowParams {
  path: string;
  // 特征专属参数
}
interface File_SidebarDragWindowResult {
  success: boolean;
  path: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn sidebardragwindow(params: ...) -> Result<File_SidebarDragWindowResult>
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
