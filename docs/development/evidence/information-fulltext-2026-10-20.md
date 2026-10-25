# 资讯正文与空详情修复

本轮在隔离配置与数据库中验证，不操作真实投资库、用户凭据、安装或远端 Git。

## 原因与实现

| 用户问题 | 核验原因 | 修复／Owner |
| --- | --- | --- |
| 新闻始终只有摘要 | 财经／股票新闻没有正文读取适配；任务只允许股票公告 | Fund `information/content.rs` 从已保存身份构造固定发布方 HTTPS 地址，只取 `ContentBody` 并转纯文本，忽略脚本／样式；现有任务支持四类来源 |
| 基金公告拿不到正文 | 代码拒绝基金来源，但公开公告接口实际返回该基金的 `AN` 正文 | 复用同一公告正文端点；原文链接定位具体 PDF，旧数字身份仍明确 unsupported |
| 长公告不完整 | 固定 `page_index=1`，忽略 `page_size`，却标 full | 顺序读取分页；20 秒总时限、单响应 2 MiB、最多 20 页、存储 64 KiB；超容量／页数标 truncated，后页错误不把首页存成 full |
| 打开后看不到信息 | 详情 GET 失败仅写首页状态栏；详情未打开，也没有原地重试 | `home-information-detail.js` 先打开 loading／错误弹窗，再加载摘要和未抓取正文；失败保留已有内容并可重试；使用 Host 的 `contentSupported`，不由 UI 猜测能力 |
| 关闭后迟到重开 | 旧实现异步完成后再次调用打开详情，缺少详情 owner | 一个 dialog 持有读取 AbortController；关闭、切换与 pagehide 撤销读取，正文任务沿现有取消／代际链；结果只更新仍连接的原 dialog |

契约先更新于 [现有 Fund 补充契约](../../contracts/fund-holding-detail-contract.md#资讯正文与详情)。存储、缓存 revision、内容版本、来源启用、取消、阅读／收藏状态沿现有 owner，无 schema／依赖变更。详情从首页 controller 分离，`ui.rs` 编译期包含生成资源；现有相关浏览器装配列表同步。中英文、纯文本换行及窄窗样式一起生成。

## 复现与检查

- `rtk env -u CARGO_TARGET_DIR cargo test -p fund-module --test information_content_test -- --nocapture`：修复前 `em_finance` 返回 400 `INFORMATION_UNSUPPORTED`，修复后四类来源复用缓存通过。初次新增测试的导入路径写错导致编译失败，修正为真实 `adapters::RawItem` 后取得上述红例，不将编译失败冒充缺陷复现。
- `rtk node scripts/modules/information-detail-browser.test.mjs`：修复前详情失败找不到 dialog 的可见错误，3 秒超时；修复后 Archive／Volt × 中英文 × 1280／320 px 共 8 组通过。覆盖读取／正文失败重试、失败保留摘要、缓存不重复抓取、安全转义、键盘、关闭后迟到读取／正文不重开、不可用原文提示；检查英文 Volt 窄窗截图。
- `information::content` 单元用例：两页拼接、后页失败不成功、容量上限、空正文、正文容器边界、段落与实体、脚本／样式／评论排除、固定端点身份校验通过。
- 显式公开实网 `cargo test -p fund-module --lib information::content::tests::fixed_publishers_actual_full_text -- --ignored --exact --nocapture`：当前发布方列表派生身份，实际财经新闻 2,448 字节／full、股票新闻 2,245 字节／full、股票公告 2,688 字节／full、基金公告抓取 70,716 字节／truncated。仅发送公开证券与文章身份，不含私人持仓或凭据。
- 显式公开实网 `cargo test -p fund-module --test information_content_test real_news_and_fund_body_task_persist_readable_text -- --ignored --exact --nocapture`：临时 Fund service → 内容任务 → 发布方 → Store → 详情查询完整运行；股票新闻保存 2,245 字节／full，基金公告 UTF-8 边界保存 65,535 字节／truncated，版本为 2，缓存有效，原文不是公告列表页。
- `rtk node scripts/modules/information-browser.test.mjs` 与 `rtk node scripts/modules/home-browser.test.mjs` 通过：资讯首次采集／缓存／刷新／禁用／隐私，首页表单／语言草稿／键盘／双主题／320～1280 px／200% 缩放均保留。旧详情失败夹具补齐实际契约的 `contentSupported=true`／`contentVersion=1`，不更改失败断言。
- 模块 UI build/check、架构规模检查、Cargo fmt 与首轮 Rust workspace（27 suites，613 passed、0 failed、12 ignored）通过；最终生成资源后复验及集成结果在下方补记。

## 范围与限制

20:44 收尾核验：最终生成资源后 Rust workspace 27 suites／613 passed／0 failed／13 ignored、Cargo fmt、`modules:ui:check` 与 `perf:check`（含 Extension／Files／Apps／安全／边界／端口）退出 0。移除全文迁移产生的无调用旧包装函数后，Fund package 复验退出 0；`git diff --check` 通过。后续用户转入取消配对需求，本段为该项独立验收快照。

- 生产页面在真实 Chrome 中检查，数据为隔离 HTTP fixture；实际发布方和 Fund service 另有实网证据，不能把两者合称完整 Chrome Native 链。现有扩展链夹具依赖的 Chrome for Testing 可执行文件在本机缺失；其他已知候选扩展加载限制仍保留，不换浏览器触发权限或降低安全设置。补充了既有 `--information-data` 场景的真实正文断言，本轮未运行该完整扩展场景。
- 大型报告仍受 64 KiB 上限，标为截断并保留原文；图片型、受限、布局不匹配或没有有效文章身份的来源明确不可用／失败，不保证任意网站全文，不绕过访问限制。
- 未进行正式安装、发布、真实用户数据修复或完整长测；不宣称整版双入口与平台验收完成。
