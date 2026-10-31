# DSH 嵌入页重复左侧栏修复（2026-10-29）

问题：从 DSH 入口进入「空间／文件／Token 统计／投资」后，页面仍显示自己的左侧侧边栏，
而 DSH 本身已有左侧导航，视觉上重复。

## 1. 复现

用隔离 Core（`NATIVES_STATE_DIR=.tmp/sidebar-state`、`NATIVES_WEB_PORT=8797`）分别以
**仓库页面**（`NATIVES_WEB_ROOT=extension`）和**产品源页面**（`~/.natives-local/product-source/ChromeExtension`）
加载 `space.html` / `files.html` / `app.html?app=fund`，附 `?standalone=1`：

| 页面来源 | `html.standalone` | `#app-sidebar` | 结果 |
| --- | --- | --- | --- |
| 仓库（当前 CSS） | 有 | 0 px | 正常 |
| 产品源（2026-10-29 01:34 快照） | 有 | **248 px** | 只剩一条空左栏（子元素被隐藏，列还在） |

产品源样式的规则只到 `html.standalone #app-sidebar .brand, … { display:none }`，
没有隐藏 `<aside>` 本身，于是保留一条空的 248 px 轨道；文件页里那条轨道还留着工作区树。
这解释了「看起来仍是模块自己的左侧侧边栏」。

## 2. 另外两个会让问题复发的成因

1. **判定过度依赖查询串**：隐藏只在 `?standalone=1` 时生效。旧 Host 进程不带该参数、
   参数被剥离或第三方嵌入时，页面立刻退回完整侧栏。
2. **单列模板留下隐式第二列**：`files.css` 里 `.toolbar`、`#status-bar` 仍写死
   `grid-column:2`。侧栏隐藏后显式网格只剩一列，这些元素把内容挤进隐式第二列，
   实测 body 计算值为 `699px 741px`，视觉上仍像「多出一条左栏」，主内容也被压窄。

## 3. 改动

- `extension/platform/standalone-boot.js`：`?standalone=1` **或** 文档被框架承载
  （`window.top !== window.self`）都进入 standalone；解析环境缺失时静默跳过。
- `extension/space.css`：`#app-sidebar` 整列隐藏不再依赖 `body.space-page`，
  同时隐藏分隔条；`body` 单列、`.space-shell` 拉满整行。
- `extension/files.css`：`.toolbar`、`.layout`、`#status-bar`、`.space-main`、`#app-stage`
  统一 `grid-column:1 / -1`，消除隐式第二列。
- `extension/apps.html` + `extension/apps.css`：应用中心同样引入 boot 脚本并隐藏 `.apps-nav`
  （虽然不在四个入口内，避免同一类回退）。
- `extension/standalone-boot.test.mjs`：新增「被嵌入」用例、`apps.html` 引入检查，
  以及四条 CSS 的嵌入布局契约（整列隐藏 + 拉回整行），防止只隐藏子元素或漏掉列占位。

## 4. 验证

同源 iframe 夹具（`extension/__embed-harness.html`，测试后删除）同时嵌入四个页面，
**不带** `standalone` 参数：

```text
space.html  cls=standalone  #app-sidebar 0px   .space-shell 1440px
files.html  cls=standalone  #app-sidebar 0px   main/.layout 1440px（原 699px）
app.html?app=fund  cls=standalone  .app-shell-nav 0px  main 1440px
apps.html   cls=standalone  .apps-nav 0px      main 1440px
对照：顶层打开 space.html（无参数） cls=""  #app-sidebar 248px
```

门禁：`npm run extension:check`（含 `standalone-boot.test.mjs`）、`standards:check`、
`architecture-check`、`git diff --check` 通过。`native-file-host` 重新构建通过。

## 5. 未验证 / 生效条件

- 未在 DSH Desktop 窗口内目视复核：本会话无法驱动该窗口。页面由 checkout 提供
  （插件以 `link:` 安装时 runtime 传 `NATIVES_WEB_ROOT=<repo>/extension`），
  重新打开模块即可看到新样式；若 DSH 仍在跑 2026-10-29 07:17 之前的 Host 运行时，
  需重启 DSH 使 `webRoot`／`standalone` 逻辑生效。
- 以 tarball 安装、走产品源页面的场景需要先刷新产品源
  （`node scripts/apps/refresh-dev-product-source.mjs`）才能拿到同一份 CSS。
- 真实数据下的内容区布局（空间卡片、文件列表、模块 iframe）只验证了列宽与无溢出，
  未做逐页视觉验收。

## 6. 追加：嵌入时移除「开合自身左侧栏」控件（同日）

侧栏隐藏后，页面上仍留着开合它的按钮，点了没有对象。逐个排查四个入口与两个模块：

| 位置 | 控件 | 嵌入时 |
| --- | --- | --- |
| `space.html` | `#space-toggle-sidebar-btn` | 已由 `html.standalone` 隐藏 |
| `files.html` | `#toggle-sidebar`（侧栏内）、`#app-stage-toggle-sidebar-btn`（悬浮） | 已隐藏 |
| `apps.html` | `#apps-toggle-sidebar-btn`（侧栏内）、`#apps-expand-sidebar-btn`（悬浮） | 补上隐藏规则 |
| 投资模块 | `#btn-toggle-left-sidebar`（页头最左，折叠的是外壳 `app-sidebar`） | **本次修复**：隐藏 + 不注册联动 |
| Token 统计模块 | 无 | — |

投资模块那个按钮特殊：它不折叠模块内部的东西，而是 `postMessage({type:'toggle-sidebar'})`
让外壳去折叠 `app-sidebar`（与空间页的 `space-toggle-sidebar-btn` 同一套协议）。宿主里外壳侧栏
已经不存在，因此：

- `modules/fund/ui/src/styles/layout.css`：`html.standalone #btn-toggle-left-sidebar{display:none!important}`；
- `modules/fund/ui/src/app.js`：`embeddedShell` 时不给该按钮注册点击、也不再向父页请求侧栏状态；
- `extension/app.js`：嵌入态忽略模块发来的 `toggle-sidebar`，防止把已隐藏的侧栏再拉起来。

判定口径修正：模块页**永远**在 iframe 里，不能用「被框架承载」判定嵌入，否则 Chrome 扩展里的
模块也会被当成宿主嵌入、把按钮一起隐藏。模块 head 只认 `?standalone=1`（由 `app.js` 在宿主
嵌入时追加），外壳页面仍保留「参数或被框架承载」双判定。

验证（同一 Core，两种载体）：

```text
未嵌入（Chrome 扩展路径）：模块 cls=""             #btn-toggle-left-sidebar display=flex  w=32
嵌入（DSH 路径）：        模块 cls=standalone      #btn-toggle-left-sidebar display=none  w=0
                                                   #btn-toggle-side（右侧看板） display=flex 保留
```

`standalone-boot.test.mjs` 增补契约：投资模块的隐藏规则与 `embeddedShell` 跳过、应用中心展开按钮、
模块 head 不得使用 framed 判定。`modules:ui:check`、`extension:check`、`apps:check`、
`standards:check`、`architecture-check`、`git diff --check` 通过。
