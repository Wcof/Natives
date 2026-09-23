# Natives

> **Natives 本地工作台** · 单一完整产品 · v0.1.0

Natives 是一个完整产品：Manifest V3 扩展提供界面，Rust/Go Native Hosts 提供本地能力，空间、文件、AI 和基金作为内置模块随同一安装包交付。正式分发尚未完成，平台签名/公证和 Windows 实机门禁仍在外部流程中。

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

手动流程只用于开发和排错，不代表正式安装：

```sh
npm install
rtk env -u CARGO_TARGET_DIR cargo build -p native-file-host

# 在 chrome://extensions 启用开发者模式，加载 extension/，复制扩展 ID
rtk node extension/install-native-host.mjs \
  --extension-id <chrome-extension-id> \
  --host-path "$(pwd)/target/debug/native-file-host"

# 打开普通 Chrome/Chromium 的 Natives 页面；进入 files.html 后 Host 按需启动
rtk node extension/launch-natives.mjs
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

正式版本由 macOS/Windows 完整产品安装器提供固定扩展、Native Hosts 和全部内置模块；首次运行按 Chrome 要求加载随包扩展目录。当前状态：

- 扩展体积估算：310,580 bytes（预算 368,640）；`native-file-host` Release：1,913,472 bytes；实测空闲 RSS：8,288 KB；EOF 后退出：9 ms。
- 10,000 项目录查询 5 样本 p95：46.73 ms（预算 50 ms）；Model Host 首次快照：702 ms、空闲 RSS：66,096 KB、空闲 CPU：0%。
- Chrome for Testing 148 E2E：newtab 成功，files 返回真实 roots 8 个、主目录 entries 24 个，console/pageerror 为空；关闭文件页后 Host 29.45 ms 退出且无残留。
- Windows 本地候选包脚本已就绪，真实 Windows 安装/升级/卸载生命周期仍待实机验证。
- 平台签名/公证、正式下载回测、跨平台生命周期 E2E 和卸载残留检查：仍是发布阻塞，不是代码闭环阻塞。

因此当前文档和脚本描述的是开发/验证流程，不宣称正式发布完成。

## 目录

```text
extension/                 # Chrome 扩展（newtab、files、Service Worker）
crates/native-file-host/   # Native Messaging Host
crates/file-manager-core/  # 文件授权与操作内核
installers/                # macOS/Windows 正式安装资产（未完成实机门禁）
docs/adr/0023-*.md         # Chrome Files 与 Native Host 决策
docs/architecture/FILE_MANAGER_AUDIT.md  # 文件能力与当前验收
```

历史架构和已删除方案只在 [`docs/archive/`](docs/archive/) 与标记为 superseded 的 ADR 中保留，不参与当前构建、门禁或产品入口。
