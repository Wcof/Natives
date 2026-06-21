# Blueprint: Term_LinuxWatchDegrade (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
Linux下fs.watch降级为非递归监听( macOS/Windows用FSEvents原生递归)，Linux递归不可靠故只监听当前目录，保证跨平台不崩。

### Reference Code Analysis
- **Target File:** `src-tauri/src/terminal/linuxwatchdegrade.rs`
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
Term_LinuxWatchDegrade:  idle → watching → debouncing → (notify | noise_discard) → watching
```
- `idle`: 初始状态
- `watching`: 文件系统监听中
- `debouncing`: 事件去抖/噪声过滤
- `notify`: 有效变更通知前端
- `noise_discard`: 噪声事件（atime/元数据/自身写入）丢弃

### Algorithm
```
// Term_LinuxWatchDegrade — Rust notify crate + debounce + stat noise filter
async fn handle_watch(paths: Vec<String>) -> Result<WatchHandle> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default())?;
    for p in paths { watcher.watch(Path::new(&p), RecursiveMode::Recursive)?; }
    
    // Debounce + noise filter loop
    let mut debounce = tokio::time::interval(Duration::from_millis(300));
    loop {
        tokio::select! {
            event = rx.recv() => {
                let e = event?;
                // stat 验证：丢弃 atime / 元数据噪声
                let meta = tokio::fs::metadata(e.paths[0]).await;
                if discarded_as_noise(&e, &meta) { continue; }
                // 3s 内自身写入过滤
                if is_self_triggered(&e) { continue; }
                emit_change(e.paths[0]);
            }
            _ = debounce.tick() => { flush_batch(); }
        }
    }
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/terminal/linuxwatchdegrade.rs` |
| 前端组件 | `src/components/terminal/` |

### Data Model
```typescript
// Term_LinuxWatchDegrade — 终端数据模型
interface Term_LinuxWatchDegradeParams {
  id: string;
  // 特征专属参数
}
interface Term_LinuxWatchDegradeResult {
  success: boolean;
  id: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn linuxwatchdegrade(params: ...) -> Result<Term_LinuxWatchDegradeResult>
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
