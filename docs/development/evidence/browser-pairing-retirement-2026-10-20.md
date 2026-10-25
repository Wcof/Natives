# 浏览器配对退役验收

2026-10-20 21:00。按用户明确指令删除配对功能，先落 [ADR-0045](../../adr/0045-retire-browser-pairing.md) 再实施；沿原单 Core、Cookie／CSRF、Surface owner，不引入无鉴权接口、持久化会话 Secret、保活轮询或新进程类型。

## 需求与实现

| 需求 | 实现／Owner | 验证 |
| --- | --- | --- |
| 删除配对 | `auth.rs` 删除 PairingCode；`server.rs` 删除生成／交换分支；`main.rs`／`service/mod.rs`／package 删除 CLI 和脚本 | 生成路由 404，配对 payload／混合字段 401，不能签发 Cookie；旧 CLI 在 3 秒预算内返回 2，不进入 Native stdin |
| 闲置后不要再配对 | Core 浏览器授权仅随显式注销、Cookie 替换或 Core 退出撤销，移除 30 分钟／12 小时期满撤销 | `browser_authorization_survives_idle_while_surface_leases_expire` 修复前红例“idle browser authorization must remain valid”；修复后模拟超过旧期限通过 |
| 不延长模块运行 | 保留 Surface 75 秒租约、hidden 释放和最后 owner 宽限；浏览器授权与资源租约分开 | 同一未来时钟测试证明旧 Surface 已清理，授权仍可创建新 Surface；跨会话／跨模块与撤销回归保持 |
| 不再弹配对窗口 | `web-entry.js` 删除 dialog／表单／验证码；无连接时仅显示非模态的 Natives 入口说明与连接重试 | Chrome 8 组均无 dialog/input/form，不夺取当前控件焦点，键盘可重试 |
| 恢复连接与刷新 | `session.js` 只保留受信启动交换与 Cookie 检查，启动授权恢复分发既有恢复事件 | 当前授权刷新成功；注销后拒绝；已启动页面经新的受信启动恢复；平台 transport／共享 SSE／Surface 更新回归通过，不重放写操作 |
| 保留安全边界 | Bootstrap 仍 256-bit／60 秒／一次消费／每分钟 10 次；Cookie 仍 HttpOnly／Host-only／Strict；Origin、Host、CSRF 和 JSON 保持 | 新增过期 Bootstrap 不影响现有授权；错误 Origin／Content-Type／额外字段拒绝；缺 CSRF 与跨会话原用例通过；真实 HTTP 限速断言通过 |

浏览器授权不因闲置失效仅指当前 Core 实例。Core 停止／重启后旧 Cookie 仍无效，正常从 Natives 本机入口重新打开会自动完成启动授权，不获取或输入配对码。未把浏览器会话写到磁盘或 Keychain，也未改模块 bearer 的 15 分钟期限。

## 检查与证据

- `rtk env -u CARGO_TARGET_DIR cargo test -p native-file-host service:: -- --nocapture`：首轮 29 项通过；随后增加短期 Bootstrap 期限及错误 Origin／JSON 类型断言。
- `rtk env -u CARGO_TARGET_DIR cargo test --workspace --no-run`／`cargo test --workspace`：27 suites，614 passed／0 failed／13 ignored，包含既有 50 次 Runtime 生命周期循环。
- 最终 `rtk env -u CARGO_TARGET_DIR cargo test -p native-file-host`：109 passed／0 failed，覆盖最后补充的期限与边界断言。Cargo fmt 通过，无本轮新增 unused 导入。
- `rtk node extension/platform/platform.test.mjs`：受信启动、共享 SSE、断连取消、跨页面 Cookie 更新及租约恢复通过。
- `rtk node scripts/web-no-pairing-browser.test.mjs`：真实 Chrome → 当前源码 Core 二进制快照 → 真实 HTTP 鉴权；临时 HOME/state/workspace/apps，只有测试空页面 shell，鉴权不使用 API 替身。Archive／Volt × 中英文 × 1280／320 px 共 8 组通过；Bootstrap fragment 清除、真实 HttpOnly／Strict Cookie、刷新／注销／恢复、非模态与键盘、已删除 CLI、原交换限速通过。已检查英文 Volt 窄窗截图。
- `rtk npm run modules:ui:build`／`modules:ui:check`：全局主题焦点规则从配对 modal 改为连接 notice，两个模块生成主题与源码一致。
- `rtk npm run perf:check`：退出 0，覆盖 Extension／Files／Apps／边界／端口／安全与 Release 编译；`standards:check`、`git diff --check` 通过。

初次浏览器夹具将新授权 URL 导航当成同文档 hash 切换，触发首次恢复的预期重载；改为正常的新文档启动，并单独测试已启动页面的恢复。矩阵原先每组执行两次授权交换，第 6 组触发现有每分钟 10 次限速；未放宽限速，额外恢复只运行一组，另显式断言第 11 次交换被拒绝。保留这些失败原因，不能当作安全规则失效或线上故障。

## 交付边界

- 未修改真实用户数据库、Cookie、Keychain、环境配置、系统注册或正在运行的 Core；未提交、推送或安装。当前开发应用重启后加载新版。
- Windows／正式安装／完整双入口业务与长测未在本轮重跑；本轮是授权流程与交互回归，不替代整版产品验收。
- 前一项资讯正文／空详情已独立收尾，详见 [资讯修复](information-fulltext-2026-10-20.md) 。
