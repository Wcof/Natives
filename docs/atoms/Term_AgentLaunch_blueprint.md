# Blueprint: Term_AgentLaunch (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
终端Agent启动按钮(Claude Code/Codex)，空闲shell就地启动，正跑任务则新开标签，launchAgent+runInDir按cwd启动。

### Reference Code Analysis
- **Target File:** `src-tauri/src/terminal/agentlaunch.rs`
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
Term_AgentLaunch:  disconnected → connecting → (connected | failed | timeout) → disconnected
```
- `disconnected`: 未连接
- `connecting`: 连接中（PTY spawn / 微信扫码 / OAuth 获取）
- `connected`: 连接成功
- `failed` / `timeout`: 连接失败或超时

### Algorithm
```
// Term_AgentLaunch — 异步连接（PTY spawn / 微信扫码 / OAuth）
async fn connect(params: ConnectParams) -> Result<Connection> {
    let shell = env::var("SHELL").unwrap_or("/bin/zsh".into());
    let system = native_pty_system();
    let pair = system.openpty(PtySize { rows: 24, cols: 80, ..Default::default() })?;
    
    let cmd = CommandBuilder::new(shell)
        .arg("-l").cwd(params.cwd)  // login shell: .zprofile/.zlogin
        .env("TERM", "xterm-256color")
        .env("LANG", "zh_CN.UTF-8");
    let child = pair.master.spawn(cmd)?;
    
    // 异步读写循环
    let reader = pair.master.take_reader();
    let writer = pair.master.take_writer();
    Ok(Connection { id: params.id, reader, writer, pid: child.pid() })
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/terminal/agentlaunch.rs` |
| 前端组件 | `src/components/terminal/` |

### Data Model
```typescript
// Term_AgentLaunch — 终端数据模型
interface Term_AgentLaunchParams {
  id: string;
  // 特征专属参数
}
interface Term_AgentLaunchResult {
  success: boolean;
  id: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn agentlaunch(params: ...) -> Result<Term_AgentLaunchResult>
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
