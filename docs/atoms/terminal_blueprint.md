# Blueprint: Terminal

## 1. Logic Deconstruction & Reference Analysis

### Experience
在桌面端内嵌真终端：多标签页、PTY直连、xterm.js渲染、Agent(Claude Code/Codex)直接在终端里跑、终端录像回放(asciinema v2 .cast)、cwd与文件树双向联动、路径可点击打开、URL可点击、微信ClawBot远程操控、合盖保持运行、Agent完成通知、退出守卫。终端是agent交互的主界面。

### Reference Code Analysis

- **Target File:** `electron/main.js` (L1-842) + `public/app.js` (L1-4572)
- **Implementation Logic:**
  - **PTY核心**: `ipcMain.handle('pty:spawn')` (L468-506) — node-pty.spawn(shellPath, ['-l'], {name:'xterm-256color', cols, rows, cwd, env}); login shell(-l)读.zprofile/.zlogin带全PATH; env强制LANG=zh_CN.UTF-8; p.onData→win.webContents.send('pty:data'); p.onExit→清理terminals Map+recStop
  - **多标签页**: 前端tab render+activate+close; 每终端独立PTY进程; 关闭kill PTY防泄漏; 每项目一个agent session常驻
  - **cwd双向联动**: `termCwdByPid()` (L712-730) — lsof -p {pid} -Fn | grep ^n; `decodeLsofPath()` (L698-711) 处理\xNN字面量; 前端点文件夹→终端cd; 终端cd→文件树定位
  - **录像**: `recStart()+recEvent()+recStop()` (L434-465) — asciinema v2 .cast格式; header含fanbox私有元信息(cwd/cols/rows/startedAt/theme); recEvent写[elapsed,code,data]; 异步写盘失败静默自废; `recPrune()` (L417-433) 保留最近60个+800MB上限
  - **录像导出**: `rec:export` (L629-663) — 本机ffmpeg转MP4/GIF; 检测不到ffmpeg优雅退回WebM; MP4偶数宽高+faststart; GIF两遍调色板
  - **Agent启动**: 空闲shell就地启动claude/codex; 正跑任务新开标签; `findAgentBin()` (L511-519) 在PATH中查找
  - **Agent检测**: `pty:proc` (L731) — node-pty process属性返回前台进程名; 判断裸shell vs claude/codex
  - **合盖保持**: `setLidIntent()` (L320) pmset -b disablesleep 1; `installSudoers()` (L268-297) 写/etc/sudoers.d免密码sudo; `refreshLidGuard()` (L305) 终端数>0禁休眠; will-quit钩子确保恢复
  - **退出守卫**: beforequit — 还有终端在跑时弹确认; quitConfirmed防二次弹窗
  - **输出尾巴**: `termTails` Map (L24) — 每终端保留~4KB去ANSI输出，给微信agent感知和完成通知摘录
  - **微信ClawBot**: `wechat:*` IPC (L768-771) — 连接弹窗状态机(未装→装了没起→网关在线→选agent→QR→扫码成功→已连接); OpenClaw网关管理; 跨终端感知termControl; 对话管理wechat:conversation/new/compact/check/send
  - **剪贴板**: `clip:image` (L508-511) nativeImage写剪贴板; `clip:file` (L512-516) AppleScript POSIX file写剪贴板
  - **拖拽落盘**: `drop:save/save-into/copy-into` (L519-557) — 无路径拖入写临时目录换路径; `uniqueDest()` (L531-536) 同名不覆盖仿访达foo 2.png
- **Pros & Cons:**
  - ✅ login shell解决GUI app找不到claude命令; UTF-8兜底防中文乱码; asciinema标准格式可互操作; 录像自动裁剪防磁盘涨; 合盖继续跑agent; 退出守卫防手滑; 废纸篓可恢复
  - ❌ node-pty需electron-rebuild原生编译; IPC中转PTY数据有JSON序列化开销; lsof cwd检测有100-200ms延迟; 常开录制有I/O开销; 微信依赖OpenClaw第三方; 合盖需sudoers管理员权限

### State Machine

```
TerminalTab:    spawning → ready → (idle | busy) → exiting → closed
AgentSession:   none → launching → running → idle → running...
Recording:      off → recording → (pruning | exporting) → off
WechatConnect:  uninstalled → installed_off → gateway_online → selecting_agent → qr_generated → qr_showing → connected
LidGuard:       sleep_normal → sleep_disabled → sleep_normal
```

### Algorithm

**PTY Spawn (Rust portable-pty):**
```rust
fn spawn_pty(id: &str, cwd: &str, cols: u16, rows: u16) -> Result<PtyHandle> {
    let system = native_pty_system();
    let pair = system.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
    let shell = env::var("SHELL").unwrap_or("/bin/zsh".into());
    let cmd = CommandBuilder::new(shell).arg("-l").cwd(cwd)  // login shell: read .zprofile/.zlogin
        .env("TERM", "xterm-256color").env("NATIVES", "1").env("LANG", "zh_CN.UTF-8");
    pair.master.spawn(cmd)?;
    Ok(PtyHandle { id: id.into(), reader: pair.master.take_reader(), writer: pair.master.take_writer() })
}
```

**Recording (asciinema v2 cast):**
```rust
fn rec_event(writer: &mut BufWriter<File>, start: Instant, code: char, data: &str) {
    let elapsed = start.elapsed().as_secs_f64();
    writeln!(writer, "[{:.6},{:?},{:?}]", elapsed, code, data).ok();
}
```

## 2. Natives Architecture

### Module Path
```
src-tauri/src/terminal/
  mod.rs            # 模块入口 + Tauri command注册
  pty.rs            # PTY spawn/resize/kill (portable-pty)
  cwd_detect.rs     # cwd检测(lsof macOS / /proc Linux / WinAPI Windows)
  recording.rs      # asciinema cast录制 + 裁剪(prune) + 导出(ffmpeg)
  agent_detect.rs   # Agent进程检测 + 二进制查找(findAgentBin)
  lid_guard.rs      # 合盖保持运行(pmset + sudoers)
  clipboard.rs      # 剪贴板(图片/文件)
  drop.rs           # 拖拽落盘(save/save-into/copy-into)
  wechat/           # 微信ClawBot(可选模块)
    mod.rs
    gateway.rs      # OpenClaw网关管理
    state_machine.rs # 连接状态机
    qr_render.rs    # QR码渲染
    term_control.rs # 跨终端感知

src/components/terminal/
  TerminalTab.tsx
  TerminalTabs.tsx
  XtermWrapper.tsx         # xterm.js封装(WebGL+Unicode11)
  AgentLaunchButton.tsx
  RecordingPanel.tsx
  RecordingPlayback.tsx
  WechatConnectDialog.tsx
```

### Data Model
```typescript
interface TerminalSession {
  id: string; cwd: string; cols: number; rows: number;
  shellPid: number; foregroundProc: string | null;
  isRecording: boolean; recordingPath?: string;
  agentStatus: 'idle' | 'busy' | 'none';
  outputTail: string;  // 最近~4KB去ANSI输出
}
interface RecordingMeta {
  name: string; path: string; size: number; mtime: number;
  width: number; height: number; cwd: string;
  startedAt: number; duration: number; isRecording: boolean;
}
type WechatState = 'uninstalled' | 'installed_off' | 'gateway_online'
  | 'selecting_agent' | 'qr_generated' | 'qr_showing' | 'connected';
```

### APIs
```rust
#[tauri::command] fn pty_spawn(id: String, cwd: String, cols: u16, rows: u16, theme: String) -> Result<PtySpawnResult>
#[tauri::command] fn pty_write(id: String, data: String)
#[tauri::command] fn pty_resize(id: String, cols: u16, rows: u16)
#[tauri::command] fn pty_kill(id: String)
#[tauri::command] fn pty_cwd(id: String) -> Result<String>
#[tauri::command] fn pty_proc(id: String) -> Result<Option<String>>
#[tauri::command] fn rec_list() -> Result<Vec<RecordingMeta>>
#[tauri::command] fn rec_read(path: String) -> Result<String>
#[tauri::command] fn rec_delete(path: String) -> Result<()>
#[tauri::command] fn rec_export(name: String, buf: Vec<u8>, format: String) -> Result<ExportResult>
#[tauri::command] fn find_agent_bin(name: String) -> Result<Option<String>>
#[tauri::command] fn set_lid_intent(on: bool) -> Result<()>
```

### Performance Design
| 策略 | 竞品(Electron) | Natives(Tauri+Rust) | 提升 |
|------|---------------|---------------------|------|
| PTY | node-pty(Electron原生模块) | portable-pty(Rust原生,零GC) | 稳定性↑ |
| 渲染 | xterm.js WebGL addon | 同xterm.js(Tauri WebView兼容) | 等价 |
| IPC | Electron ipcMain/ipcRenderer JSON | Tauri Event::emit字节流 | 2x |
| 录制写盘 | fs.createWriteStream | tokio::fs::BufWriter异步 | 1.5x |
| cwd检测 | lsof子进程(100-200ms) | Rust /proc/PID/cwd直接读(<1ms) | 100x |
| 输出尾巴 | Map存4KB字符串 | 同方案,Rust String更高效 | 等价 |
| Agent检测 | p.process属性 | portable-pty+ps/proc补 | 等价 |

## 3. Risk Analysis

### Risks
1. **node-pty → portable-pty迁移**: API差异大, portable-pty无process_name()等价物, 需/proc或ps补
2. **xterm.js兼容性**: Tauri WebView2(Windows)/WKWebView(macOS)对WebGL支持程度不一
3. **lsof cwd检测**: macOS特有, Linux用/proc/PID/cwd, Windows需OpenProcess+QueryFullProcessImageName
4. **录像回放**: asciinema格式前端xterm.js播放器需验证Tauri WebView兼容
5. **微信ClawBot**: 依赖OpenClaw/iLink第三方, Tauri无Electron的child_process便利, 需Rust Command替代
6. **合盖保持**: macOS pmset需sudoers, Tauri打包后获取管理员权限流程不同
7. **CapsLock补丁**: xterm vendor补丁需验证Tauri WebView下是否仍需

### Mitigation
- PTY: portable-pty为主, /proc/ps补进程检测; 保留node-pty fallback(通过sidecar)
- xterm.js: 预检测WebGL支持, 不支持降级Canvas渲染
- cwd检测: 条件编译#[cfg(target_os)], macOS用lsof, Linux读/proc, Windows用WinAPI
- 微信: 初版可跳过, 后续通过Tauri sidecar或Rust Command实现
- 安全: Tauri窗口CloseRequested事件替代Electron beforeunload, 更可靠
