# brand-spec.md · Natives

宣传片涉及的品牌资产。全部来自仓库，不凭记忆。

## 1 · 品牌名与产品定位

- 中文字标：**Natives 本地工作台**
- 英文：Natives
- 版本：v0.1.0（`README.md` 首段）
- 定位：一个完整产品——Manifest V3 扩展提供界面，Rust/Go Native Hosts 提供本地能力，空间、文件、AI、投资作为内置模块随同一安装包交付。

## 2 · 标志

- 现有标记：深色圆形底 + 白色折线闪电（近似字标 N 的折角），见 `docs/img/首页-浅色主题.png` 左上角。
- 仓库内**没有**可用的矢量 logo 源文件（`extension/manifest.json` 的 icons 指向 `icons/folder-*.png`，那是文件夹图标，不是品牌标）。
- 处理：按原标记重绘为 SVG——圆形底 + 单笔画折线。片中使用两版：
  - 纸面（Archive）：`#202733` 圆底 + `#F5F6F8` 折线
  - 黑场（Volt）：`#CDF24B` 圆底 + `#0B0C0A` 折线
- 英文标字：系统无衬线 `-apple-system` 600，字距 -0.03em（对齐产品侧栏字标）。

## 3 · 颜色（唯一来源：`extension/theme.css`）

Archive（默认，纸面）：

| 角色 | 值 |
|---|---|
| 底色 | `#F5F6F8` |
| 表面 | `#FFFFFF` |
| 主文字 | `#202733` |
| 次文字 | `#5B6677` |
| 边界 | `#E5E9EF` |
| 强调 | `#345780` |
| 强调（深） | `#29476B` |

Volt（深色，黑场）：

| 角色 | 值 |
|---|---|
| 底色 | `#0B0C0A` |
| 表面 | `#131410` |
| 主文字 | `#F2F2EA` |
| 次文字 | `#9FA190` |
| 边界 | `#262920` |
| 强调 | `#CDF24B` |
| 强调（亮） | `#DDFF69` |

> 注：`#DDFF69` 为 `--natives-accent-strong`，片中不使用；只用 `#CDF24B`。

## 4 · 字体

- 产品字体栈（`--natives-font`）：`-apple-system, BlinkMacSystemFont, "PingFang SC", "Segoe UI", Roboto, sans-serif`
- 等宽（`--natives-font-mono`）：`ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`
- 片中新增一层：中文衬线 `Songti SC` 用于品牌叙述句（人声），与产品的无衬线界面形成两轨。这条是导演决定，不冒充产品规范。

## 5 · 片中使用的真实技术事实（逐条可溯源）

| 片中表述 | 依据 |
|---|---|
| 扩展页面在受限沙箱 iframe 里，通过 Native Messaging 连本机进程 | `AGENTS.md`「Target architecture」段 |
| `connectNative` + `app:handshake` / `app:start` | `AGENTS.md` 同段（App Runtime Protocol v2） |
| 单一 Native Host 注册名 `com.natives.app_runtime` | `AGENTS.md` 同段 |
| App Runtime 绑 `127.0.0.1:0`，端口由 `app:start` 上报；8765 已退役 | `AGENTS.md` + ADR-0035 |
| 服务只监听 `127.0.0.1`，不监听 `0.0.0.0` | `README.md`「一键启动」 |
| 凭据由可信 Extension 表单写入 OS Keychain，模块 iframe 不接收明文 | `README.md`「财务设置」段 |
| Model Host 直连用户配置的供应商，不再有代理/路由 | `README.md` + ADR-0044 |
| 内置模块：空间 / 文件 / AI（model-host）/ 投资（fund） | `README.md` 首段 + `modules/` |
| 用户目录 `~/.natives/apps/<appId>/` 只放 `activation.json`、`data/`、`imports/`、`cache/`、`logs/` | `AGENTS.md` 同段 |

## 6 · 不使用

- 不用 `docs/img/*.png` 里旧版珊瑚色截图的配色（已被 `theme.css` 的 Archive/Volt 取代）。
- 不编造用户数、调用量、收益等运营数字。片中唯一数字是结构性的「四个模块」。
- 不用第三方 logo、不用 emoji、不用渐变与发光。
