# DSH 嵌入页配色对齐（2026-10-29）

问题：嵌入 DSH 主面板后，Natives 页面的浅色与深色与 DeepSeek Harness 的色系不搭——
浅色偏冷灰蓝、深色是带绿调的黑配荧光柠檬绿强调，放在 DSH 的中性界面里很突兀。

## 1. 宿主实际色板（实测）

从 DSH 运行时 `body` 读取 `--dsw-static-neutral-bluish-*` 与 `--dsw-alias-*`：

| 角色 | 浅色（无 `data-ds-dark-theme`） | 深色（`data-ds-dark-theme`） |
| --- | --- | --- |
| 底 | `#fff`（bg-base） | `#151517`（bg-base） |
| 层 | `#fff` / `#fff` / `#fff` | `#232324` / `#2c2c2e` / `#353638` |
| 正文 | `#0f1115` | `#f9fafb` |
| 边框 | `rgba(0,0,0,.04/.10/.12)` | `rgba(255,255,255,.06/.12/.16)` |
| 品牌／主按钮 | `#0f1115` | `#f9fafb` |
| 危险／成功 | `#ec1313` / `#22c55e` | `#f25a5a` / `#22c55e` |
| 遮罩 | `rgba(0,0,0,.24)` | `rgba(0,0,0,.5)` |

中性阶（neutral-bluish）：`#fff, #f9fafb, #ebeef2, #e9ecf2, #e1e5ee, #cfd3d6,
#adb2b8, #979da6, #81858c, #61666b, #353638, #1b1b1c, #151517, #0f1115`。
DSH 的品牌是**单色**（浅色近黑、深色近白），没有彩色强调。

## 2. 改动

- `extension/theme.css`：新增 `html.standalone` 与 `html.standalone[data-theme="volt"]`
  两段，把全部基础角色重映射到上表；`archive`／`volt` 原定义不动，主题值仍只有两个
  （R-U1），布局与行为零改动。
- `extension/app.js`：嵌入时给模块 iframe 的 URL 追加 `&standalone=1`（与 `theme=` 并列）。
- `modules/fund/ui/src/partials/head.html`、`modules/tokenusage/ui/src/index.html`：
  head 内联脚本在首帧前按 `?standalone=1` 或 `window.top !== window.self` 打
  `html.standalone`；Token 统计页此前没有主题内联脚本，一并补上 `?theme=` 消费，
  深色不再是亮底。
- 重建 `modules/*/ui/dist`、`natives-app-runtime`、`native-file-host`，
  并刷新 `~/.natives-local/product-source`（模块 UI 与 runtime 都编译进二进制，
  只改源码不重建不会生效）。

## 3. 验证

带 token 的同源嵌入夹具（四页 + 两个模块 iframe，`archive`／`volt` 各一轮）：

```text
ARCHIVE  space/files/app?app=fund/app?app=tokenusage  --natives-bg #f9fafb  accent #0f1115
         模块 iframe ?theme=archive&standalone=1      #f9fafb        #0f1115
VOLT     space/files/app?app=fund/app?app=tokenusage  #151517        #f9fafb
         模块 iframe ?theme=volt&standalone=1         #151517        #f9fafb
```

截图核对：深色无绿调、无柠檬绿强调；浅色中性白、主按钮为近黑（与 DSH 主按钮一致）；
投资与 Token 统计页在两种模式下与外壳同色。门禁：`modules:ui:check`、`extension:check`、
`standards:check`、`architecture-check`、`git diff --check` 通过。

## 4. 未验证 / 注意

- 未在 DSH Desktop 窗口内目视复核；夹具用强制 `data-theme` 覆盖了「未登录时页面不走
  主题应用」的路径，真实入口有会话，主题由 `?theme=` 正常生效。
- `chrome` 扩展与本机 Web 仍是原 Archive（蓝）／Volt（荧光绿），这是有意的：只在
  宿主内贴合 DSH。
- 对比度按 DSH 取值，未逐控件跑 WCAG；若后续发现次要文字偏灰，可只调
  `--natives-text-muted`。
