# Natives

> **Chrome Files Workspace** · Natives 本地文件工作台 · v0.1.0

Natives 当前可运行的文件产品由 Manifest V3 扩展和按需 Rust Native Messaging Host 组成：新标签页提供轻量入口，文件页直接管理本地文件。正式分发尚未完成，CWS ID、平台签名/公证和 Windows 实机门禁仍在外部流程中。

## 当前文件产品

```text
Chrome/Chromium
  ├─ newtab.html  → 轻量工作台入口
  └─ files.html   → 直接 connectNative
                    → crates/native-file-host
                      → crates/file-manager-core
                        → 本地文件系统
```

`newtab.html` 不读取文件系统、不创建 Native Port。Service Worker 只处理工具栏入口；不持有 Native Port、不轮询、不保活。关闭文件页后 Chrome 关闭 stdio，Host 在 EOF 时清理 watcher/后台任务并退出。

## 开发安装与启动

`npm run dev` 是唯一的开发入口：它会先停止本项目已有的 Debug/Release Host，再构建 Debug Host、生成/更新 `dist/dev-extension` 并持久注册；Chrome 在扩展发起 Native Messaging 连接时按需启动新 Host，按下 `Ctrl+C` 时开发 Host 也会被清理。该命令不会启动浏览器。

```sh
npm run dev
```

首次运行只需要 Rust 工具链；扩展目录始终是 `dist/dev-extension`。在 Chrome 中手动加载或重新加载该目录后，页面会使用已注册的 Native Host。

旧的手动流程仍可用于排错或人工加载扩展，不代表正式发布安装：

```sh
npm install
rtk env -u CARGO_TARGET_DIR cargo build -p native-file-host

# 在 chrome://extensions 启用开发者模式，加载 extension/，复制扩展 ID
rtk node extension/install-native-host.mjs \
  --extension-id <chrome-extension-id> \
  --host-path "$(pwd)/target/debug/native-file-host"

# 打开普通 Chrome/Chromium 工作台新标签；进入 files.html 后 Host 按需启动
rtk node extension/launch-workbench.mjs
```

重注册时重复执行安装命令；移除开发 Host（不删除扩展）：

```sh
rtk node extension/install-native-host.mjs --uninstall
```

完整排错与平台注册位置见 [`extension/README.md`](extension/README.md)。日常不需要手动启动或关闭本地服务。

## 构建与检查

```sh
# Release Host、扩展门禁与文件性能检查
npm run extension:release

# 扩展与 Native Host 检查
npm run extension:check
npm run perf:check
```

Native Messaging 支持目录/搜索分页与取消、文件预览和写入、复制/移动/重命名、废纸篓、打开/定位及目录变更通知。路径规范化、敏感目录防护和操作白名单由 Rust Host 负责；Renderer 不直接访问任意本地路径、SQLite、进程或 Secret。

## 正式安装状态

未来正式版本将由 Chrome Web Store 提供稳定扩展 ID，由 macOS/Windows 原生安装器自动注册 Native Host；用户只需完成 Chrome 的一次扩展启用确认。当前状态：

- 扩展体积估算：16,304 bytes（estimate）/ 33,825 bytes（raw）；Host：1,074,144 bytes；实测 RSS：6,032 KB；EOF 后退出：4 ms。
- macOS unsigned development pkg 最新实测：446,686 bytes；签名后体积会变化，不代表正式分发体积。
- 10,000 项目录首 100 项 5 样本 p95：25.6456 ms；macOS 连续 5 次 idle CPU：0.3%，RSS：6,032 KB。`otool` 未发现 Metal/OpenGL/WebKit 链接（仅说明无 GPU 框架链接，不等于系统 GPU 采样）。
- Chrome for Testing 148 E2E：newtab 成功，files 返回真实 roots 8 个、主目录 entries 24 个，console/pageerror 为空；关闭文件页后 Host 29.45 ms 退出且无残留。
- Windows：仅有静态/IExpress 安装资产，尚未实机验证。
- CWS ID、签名证书、公证、跨平台生命周期 E2E 和卸载残留检查：仍是发布阻塞，不是代码闭环阻塞。

因此当前文档和脚本描述的是开发/验证流程，不宣称正式发布完成。

## 目录

```text
extension/                 # Chrome 扩展（newtab、files、Service Worker）
crates/native-file-host/   # Native Messaging Host
crates/file-manager-core/  # 文件授权与操作内核
installers/                # macOS/Windows 正式安装资产（未完成实机门禁）
docs/adr/0023-*.md         # Chrome Files 与 Native Host 决策
docs/architecture/FILE_MANAGER_AUDIT.md  # 文件能力与迁移证据
```

旧 Tauri/AI Workspace、Agent、Jobs、Workshop 等代码属于迁移期 Legacy，不是当前文件产品入口；其历史架构与删除边界见 [`docs/README.md`](docs/README.md) 和 [`docs/adr/0020-ai-native-personal-workspace-rearchitecture.md`](docs/adr/0020-ai-native-personal-workspace-rearchitecture.md)。

## 许可证

[MIT](LICENSE)
