# Natives Chrome 扩展

当前版本将 Chrome 新标签页替换为轻量 `newtab.html`；文件入口和工具栏文件夹按钮打开独立的 `files.html`。只有文件页按需连接 Rust Native Messaging Host；Service Worker 不持有 Native Port，也不需要每天手动启动本地服务。

## 双击预览页面样式

可在 Finder 中直接双击 `extension/files.html`。通过 `file://` 打开时，同一页面会自动进入带示例文件的静态预览模式，用于检查布局、三套皮肤、列表/网格、图标尺寸和预览面板；底栏会明确显示“静态预览 · 未连接本地磁盘”。静态预览不会读取或修改磁盘，也不能代替扩展模式的 Native Messaging、真实文件操作和生命周期验收。

扩展构建使用仓库根目录的 `npm run extension:release`；Tauri/AI Workspace target 属于迁移期 Legacy，不作为文件扩展发布链路。

## 一键开发（macOS/Linux）

```sh
npm run dev
```

该命令构建 Debug Host，并用 Node 标准库生成/更新 `dist/dev-extension`，持久写入 Native Host 清单后保持运行，直到按下 `Ctrl+C`；不会启动浏览器。Chrome 在扩展连接时按需启动 Host。

`Ctrl+C` 移除临时 Native Host 注册但不会关闭浏览器。浏览器仍开着时不能删除它的临时 Profile；关闭文件页面后 Chrome 会断开端口，已有 Host 随 EOF 退出。

Windows 的 Native Messaging 只能由 HKCU 注册表发现，无法安全隔离到临时 Profile；请使用下方手动流程或安装器资产。

## 手动开发安装（macOS/Linux/Windows）

这是未打包扩展的开发流程，不代表正式发布安装。开发者需要在 `chrome://extensions` 加载扩展，并使用本机扩展 ID 注册 Host。

1. 构建 Host：

   ```sh
   rtk env -u CARGO_TARGET_DIR cargo build -p native-file-host
   ```

2. 在 `chrome://extensions` 加载 `extension/`，复制扩展 ID，然后运行注册脚本（会自动创建系统目录并写入清单）：

   ```sh
   node extension/install-native-host.mjs \
     --extension-id <chrome-extension-id> \
     --host-path "$(pwd)/target/debug/native-file-host"
   ```

   重新注册时重复执行上述命令；卸载 Host（不会删除扩展）执行：

   ```sh
   node extension/install-native-host.mjs --uninstall
   ```

   也可以手动编辑 `native-host-manifest.json`，将 `path` 改为 Host 绝对路径并替换扩展 ID。
3. 手动注册时，将清单放入 Chrome Native Messaging 目录：
   - macOS：Chrome 使用 `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/`，Chromium 使用 `~/Library/Application Support/Chromium/NativeMessagingHosts/`
   - Linux：Chrome 使用 `~/.config/google-chrome/NativeMessagingHosts/`，Chromium 使用 `~/.config/chromium/NativeMessagingHosts/`
   - Windows：运行注册脚本；它会写入 Chrome 和 Chromium 的当前用户注册表项。
4. 打开 `chrome://extensions`，启用开发者模式，加载 `extension/` 未打包扩展。
5. 点击工具栏的“文件管理”按钮，或在新标签页点击文件入口；打开文件页时 Chrome 会按需启动 Host，关闭页面后由 EOF 生命周期退出。

## 正式安装（尚未发布）

正式版本将由 Chrome Web Store 提供扩展和稳定 ID，由 macOS/Windows 原生安装器自动注册 Native Host；用户只需完成 Chrome 的一次扩展启用确认。CWS ID、签名证书、公证和 Windows 实机验证尚未完成，当前不能宣称正式发布已完成。

## 排错

- 页面空白或显示 `ERR_FILE_NOT_FOUND`：先在 `chrome://extensions` 点击“重新加载”，确认加载的是包含 `manifest.json`、`files.html`、`files.js` 和 `files.css` 的 `extension/` 根目录，再通过工具栏按钮打开页面；不要把仓库路径直接当作普通网页地址打开。
- Native Host 连接失败：复制扩展详情页显示的 32 位 ID，重新执行注册命令，并确认 `--host-path` 是可执行的绝对路径。Chrome 可在 `chrome://extensions` 的 Service Worker 检查器查看连接错误，Chromium 使用相同流程。
- 修改 Host 路径或扩展 ID 后，先运行 `node extension/install-native-host.mjs --uninstall`，再重新注册，最后重载扩展。

## 从桌面启动工作台

如果希望保留 Chrome 的正常标签栏，只自动打开一个工作台标签页：

```sh
node extension/launch-workbench.mjs
```

注册脚本会自动保存扩展 ID，所以后续不需要再传参数。macOS 可设置 `NATIVES_BROWSER=Chromium` 选择 Chromium；Linux 和 Windows 会按顺序查找已安装的 Chrome/Chromium。把这条命令制作成桌面快捷方式后，双击即可打开普通 Chrome 窗口并新建工作台标签。

Native Host 只接受白名单扩展通过 Chrome Native Messaging 发来的结构化方法：`version`、`roots`、`volumes`、`list_dir`、`search`、`search_cancel`、`stat`、`read_file`、`image_preview`、`archive_list`、`preview_cancel`、`create_folder`、`create_file`、`write_file`、`rename`、`copy`、`move`、`duplicate`、`copy_batch`、`move_batch`、`trash`、`trash_batch`、`batch_cancel`、`open`、`reveal`、`watch_start`、`watch_stop`、`import_probe`、`import_begin`、`import_chunk`、`import_end`、`import_cancel`。`list_dir`/`search` 支持 `offset`/`limit` 和 `hasMore`（单次最多 2000 项），搜索框输入 `content:关键词` 可按受控文本内容匹配（单文件上限 2 MiB，并返回最多 5 条带行号上下文）；写入支持 `expectedMtime` 冲突检测，搜索、预览、批量任务和导入可通过 request ID/上传 ID 取消，目录监听通过 `notify` 推送标准化 `fs_changed` 事件。压缩包仅读取 ZIP 中央目录，不解压；导入使用 512 KiB 分块和自动编号冲突策略；图片预览仅允许受控格式与大小。路径校验和敏感目录拒绝复用现有 `file_manager` 授权内核。
协议补充：当前 Host 还提供 `locate`（安全文件名定位）、`editor`（VS Code 或默认编辑器打开）、`copy_paths`（复制路径）、`copy_image`（复制受控图片）和 `disk_usage`（受限目录占用统计），均经过参数白名单与路径授权校验；不提供终端或任意进程接口。
