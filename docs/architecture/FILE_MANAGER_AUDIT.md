# 文件管理模块当前状态

> 更新：2026-09-14。本文只描述当前生产链；迁移期对照和旧实现记录位于
> `docs/archive/legacy-architecture/FILE_MANAGER_AUDIT.md`。

## 生产入口

```text
extension/newtab.html ─┐
extension/files.html ──┴─ NativeClient ── native-file-host ── file-manager-core
```

- `newtab.html` 只提供空间和文件入口，不建立 Native Port。
- `files.html` 按需持有一个 Native Messaging Port；`pagehide`、隐藏空闲和断线都会释放它。
- Service Worker 无 Native Port、轮询或保活职责。
- Host 只接受受限文件方法，路径授权由 `file-manager-core` 统一执行；页面不能传任意进程、数据库或 Secret 请求。

## 当前能力

- 目录分页、列表/网格、排序、隐藏文件、搜索、收藏、最近记录和受控目录树。
- 新建、写入、重命名、复制、移动、批量操作、废纸篓、归档、导入和 Watcher。
- 文本、代码、Markdown、JSON、媒体、图片、PDF、CSV 与归档只读预览；编辑保存使用 mtime 乐观锁。
- 所有高频请求有大小、并发、取消和超时边界；归档与预览拒绝路径穿越、符号链接越权和伪造 MIME。
- 中英文 locale 必须同步；空间主题只通过现有 Extension token 切换。

## 必须保持的资源约束

| 指标 | 目标 |
|---|---:|
| Extension 发布体积 | ≤ 360 KiB |
| Host Release 体积 | ≤ 4 MiB 过渡预算 |
| 10,000 项目录首屏 p95 | ≤ 50 ms |
| 最后一个文件页关闭到 Host 退出 | ≤ 2 s |
| Watcher 事件合并窗口 | 250 ms |
| 单次预览文本 | ≤ 64 KiB |

超出预算先补可复现实测和根因，再修改共享实现；禁止重新引入第二套页面或后台服务。

## 验收

```sh
rtk npm run extension:check
rtk npm run perf:files
rtk env -u CARGO_TARGET_DIR cargo test -p native-file-host
```

真实 Chrome、Windows 和签名安装证据单独标注为平台验收；本地 Harness 不能替代真实浏览器结果。
