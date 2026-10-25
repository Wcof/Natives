# ADR-0045：移除浏览器配对与闲置授权失效

- 状态：Accepted（2026-10-20，用户明确要求删除配对功能）
- 范围：修订 ADR-0042 的配对码与网页会话期限；模块 bearer、Surface 租约与产品进程边界不变。

## 问题

已通过 Natives 本机入口授权的网页，在 30 分钟闲置或 12 小时后失效，再次访问会弹出要求执行 CLI 获取验证码的模态。这个流程给正常的本地使用增加了反复配对步骤。

## 决定

1. 删除配对 UI、手输配对码交换、生成接口、`--pair` CLI 及 `web:pair` 脚本。浏览器不会自动弹出配对窗口。
2. 首次访问仍由受信本机 Launcher／`--launcher-web` 发放 256-bit、60 秒、一次性 Bootstrap Token，立即清除 fragment 并换取 HttpOnly、Host-only、SameSite=Strict 的浏览器会话。普通网页不能自行获得授权。
3. 已授权浏览器会话在当前 Core 实例内不因闲置／固定时长失效；显式注销、替换同 Cookie 会话、Cookie 清除或 Core 退出仍撤销。维持 64 会话、128 Surface、单会话 32 Surface 的容量限制；不持久化会话 Secret、不新增保活轮询或守护进程。
4. Core 重启后旧 Cookie 仍无效；页面显示非模态的“从 Natives 重新打开”及重试连接入口，用户无需获取或输入配对码。恢复后已启动页面释放旧 Surface、重读安全投影，不自动重放写操作；首次未启动页面恢复后重新加载。
5. Cookie、CSRF、Host、精确 Origin、JSON 类型、限流、短期模块 bearer、Surface 75 秒租约、hidden 释放及最后 owner 60 秒宽限保持。移除闲置授权过期不代表让未授权请求通过，也不延长模块运行。

ADR-0047 补充网页 document 连接：隐藏只释放业务 Surface，保留每页唯一被动连接以便返回时按需启动；pagehide 后释放，所有 owner 离开后 Core 仍退出。

## 验收

- 模拟超过旧闲置／绝对期限，浏览器授权仍有效，旧 Surface 已回收且可创建新 Surface。
- 注销、跨会话 Surface、缺失／错误 CSRF、跨域与未知 Cookie 仍拒绝；Bootstrap 仍一次性、限时、限流。
- 配对码 payload 与生成路由拒绝，删除 CLI 不落入 Native stdin 等待；源码、包、文案及现行命令无可用配对入口。
- 真实 Chrome 覆盖已授权、未授权、连接恢复、中英文／主题／窄窗与键盘；Rust 与最终集成门禁通过，实际安装与平台项单列。
