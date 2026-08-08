# H0 Gate Decision — html preview relative-resource loading

- **TASK_ID**: t03
- **RUN_ID**: 20260808-175539
- **PARENT_SHA**: 91d997e1245bba1fedaac5010cfe9ce468e3cb58
- **BRANCH**: agent/resource-preview-v2/20260808-175539/t03-html-spike
- **工作树**: `Natives-wt-resource-v2-20260808-175539/t03-html-spike`
- **日期**: 2026-08-08

## 1. 三态结论

> **H0 = BLOCKED** — 原因：**无真实 Tauri/WebKit headed evidence**。

H0 的定义：在真实 Tauri/WebKit **headed** 窗口中渲染 html preview，取得"相对资源
（`../`、中文+空格文件名、srcset、CSS `url()`、ES module `import`/`fetch`、symlink
逃逸、encoded traversal）加载行为"的第一手 WebKit 渲染证据。

### 1.1 环境事实核验（2026-08-08）

| 事实 | 核验结果 |
| --- | --- |
| macOS Aqua GUI 会话 | ✅ `launchctl managername` = `Aqua`（GUI 会话存在） |
| WebKit.framework | ✅ `/System/Library/Frameworks/WebKit.framework` 存在 |
| cargo | ✅ `cargo 1.96.0` 存在（`/Users/ldh/.cargo/bin/cargo`） |
| DISPLAY 环境变量 | ❌ 未设置（空字符串） |
| 已构建 Tauri 应用 | ❌ `src-tauri/target/` 不存在（从未 cargo build 过） |
| 前端产物 / 依赖 | ❌ `out/` 缺失（`frontendDist: "../out"`）；`node_modules` 缺失 |

### 1.2 为什么是 BLOCKED 而不是 PASS / FAIL

- 产出真实 headed evidence 的必要路径是：完整 `cargo build` 整个 workspace
  （`src-tauri` + `crates/*` + `src-agent-daemon`，Cargo.lock ~179KB 依赖树，
  预估 10+ 分钟），且启动 Tauri 需要前端产物 `out/` 与 `npm` 依赖
  （`beforeDevCommand: npm run daemon:build && npm run web:dev`），当前
  `node_modules` 未初始化。
- 本任务预算（单次 spike 执行）内无法完成上述构建与 headed 窗口启动，
  **无法取得真实 WebKit 渲染证据**。
- 按规则：**没有真实 headed evidence → 必须标注 BLOCKED，禁止 fake PASS**。
  没有任何构建/启动行为在本次执行中发生，因此也不构成 FAIL（尚无被证伪的实现）；
  更准确的状态是"被阻塞"，等待具备构建预算的后续 gate 补齐 headed evidence。

## 2. H0 Fixture 清单与验证命令

新增（全部位于 `spikes/html-preview-case/`，未触碰 `src/`、`src-tauri/`）：

```
spikes/html-preview-case/
├── outside-secret.txt                     # 'TOP-SECRET-OUTSIDE'（越界探针目标）
└── root/
    ├── index.html                         # 根页面：../ 引用、srcset、内联 script、
    │                                      #   <script src>, <script type=module>,
    │                                      #   <video>, fetch('data/demo.json')
    ├── link-out -> ../outside-secret.txt  # 符号链接（root 逃逸探针）
    ├── pages/nested.html                  # 合法 ../ 引用（../img/中文 空格.png、
    │                                      #   ../styles/nested/more.css）
    ├── styles/site.css                    # url('../img/中文 空格.png') 相对引用
    ├── styles/nested/more.css             # url('../../img/中文 空格.png') 相对引用
    ├── js/main.js                         # 普通脚本
    ├── js/module.js                       # ES module：import './main.js'、
    │                                      #   fetch('../data/demo.json')
    ├── data/demo.json                     # JSON 资源
    ├── img/中文 空格.png                  # 空文件（中文+空格文件名）
    ├── media/a.mp4                        # 空文件
    └── encoded/
        └── ..%2F..%2Foutside-secret.txt   # encoded traversal 文件名探针
```

### 2.1 验证命令

```bash
# 结构清单
ls -laR spikes/html-preview-case/

# 符号链接目标（应输出 ../outside-secret.txt）
readlink spikes/html-preview-case/root/link-out

# 越界内容可达性（真实读取，应输出 TOP-SECRET-OUTSIDE）
cat spikes/html-preview-case/root/link-out

# 关键文件字节数
wc -c spikes/html-preview-case/outside-secret.txt \
      "spikes/html-preview-case/root/img/中文 空格.png" \
      spikes/html-preview-case/root/media/a.mp4

# encoded traversal 文件名存在性
ls -la spikes/html-preview-case/root/encoded/
```

本步已执行的实机结果：`readlink` → `../outside-secret.txt`；
`cat root/link-out` → `TOP-SECRET-OUTSIDE`；两个媒体/图片文件为 0 字节。

### 2.2 已执行证据（2026-08-08 复核，逐条实机输出）

下列命令在 integration 树内真实执行，输出原样固化，作为 H0 fixture 真实性的可复核证据：

```text
$ git ls-files spikes/html-preview-case/            # fixture 全部入库
"spikes/html-preview-case/root/img/中文 空格.png"
spikes/html-preview-case/outside-secret.txt
spikes/html-preview-case/root/data/demo.json
spikes/html-preview-case/root/encoded/..%2F..%2Foutside-secret.txt
spikes/html-preview-case/root/index.html
spikes/html-preview-case/root/js/main.js
spikes/html-preview-case/root/js/module.js
spikes/html-preview-case/root/link-out
spikes/html-preview-case/root/media/a.mp4
spikes/html-preview-case/root/pages/nested.html
spikes/html-preview-case/root/styles/nested/more.css
spikes/html-preview-case/root/styles/site.css

$ git ls-files -s spikes/html-preview-case/root/link-out   # 符号链接（120000）
120000 89f9dc8998e232899a39a0a1a4cf202633b8cb39 0  spikes/html-preview-case/root/link-out

$ readlink spikes/html-preview-case/root/link-out          # root 外逃逸目标
../outside-secret.txt

$ cat spikes/html-preview-case/outside-secret.txt          # 越界 secret 真实存在
TOP-SECRET-OUTSIDE

$ ls -la spikes/html-preview-case/root/encoded/            # encoded traversal 文件名
..%2F..%2Foutside-secret.txt

$ grep -o '\.\./img/中文 空格.png\|\.\./styles/nested/more.css' \
    spikes/html-preview-case/root/pages/nested.html | sort -u   # 合法 ../ 引用
../img/中文 空格.png
../styles/nested/more.css
```

三态结论（与 §1 一致）：**H0 = BLOCKED**。fixture 真实性证据齐备（root 外
secret、symlink escape、encoded traversal、合法 `../`、CJK 空格路径、nested
CSS/module/fetch）；但本机无真实 Tauri/WebKit headed evidence（`src-tauri/target`
从未构建、`out/` 不存在），未执行任何浏览器级相对资源加载验证 → 不得 PASS，
也不构成 FAIL（尚无被证伪的实现），按规则记 **BLOCKED**，只冻结 HTML lane
（T20/T21/T15），不阻塞 Markdown/JSON/Media/Browser/Host IO。

## 3. 候选方案对比（H0 BLOCKED 下的倾向性分析，纯文档，不实现）

| 维度 | A. srcDoc + parser rewrite（现状 `src-tauri/src/html_preview.rs`） | B. opaque scoped served-root |
| --- | --- | --- |
| 机制 | 读取 HTML 文本，正则/扫描改写 `src/poster/href` 为 `/fs/` 代理 URL，`srcDoc` 注入 sandbox iframe（无 `allow-same-origin`），另起 tiny_http server | 把 HTML 所在根目录作为**不透明作用域根**，由内部 server/scheme 按相对路径原生解析资源（`http://asset.localhost` 或 scoped protocol），iframe 仍 sandbox |
| 相对路径 | 依赖文本改写覆盖率 | 交给 WebKit 原生解析，天然支持 |
| `srcset` / CSS `url()` / `<source>` / `@import` / JS `fetch` / module `import` | 现状改写器只覆盖 `src/poster/href` 字符串属性，**srcset 候选串、CSS url()、import/fetch 均不重写** → 中文空格文件名、`../` 逃逸、`..%2F` 编码变体易漏 | 所有相对引用走同一 scoped 根解析，覆盖完整 |
| 越界防护 | 依赖 `is_path_allowed`（canonicalize + `starts_with`）在 server 侧收口；symlink/encoded 需逐项验证 | 由 scoped-root 的路径规范化（`..`/`%2F`/symlink realpath）统一收口，策略集中 |
| 对 H0 fixture 的敏感点 | `img srcset`、`url('中文 空格.png')`、module `fetch('../data/demo.json')`、`link-out` symlink、`..%2F` 文件名大概率失败或未覆盖 | 这些探针正是方案 B 设计要原生支持的场景 |
| 实现成本 | 已有实现，但需持续补漏（改写得越多越脆弱） | 需要新实现（scoped root 路由 + 路径规范化），一次性投入 |
| 依赖 | 需要本机起 HTTP 端口，CSP/connect-src 需放行 | 同样需要 scoped 资源端点；CSP 可收窄到 scoped origin |

### 3.1 倾向性（H0 BLOCKED 前提下）

- **仅作倾向，不作定论**：本 gate 无真实 headed evidence，任何"方案 A 已足够 / 方案 B
  更安全"的断言都未经 WebKit 渲染验证，因此**不在本文档做最终选型**。
- 基于代码走读的倾向：方案 A（现有 parser rewrite）对 srcset / CSS `url()` /
  module `import`/`fetch` / symlink / encoded traversal 的覆盖存在明显空洞，而 H0
  fixture 正是为验证这些空洞而设计；方案 B（opaque scoped served-root）让相对路径
  由 WebKit 原生解析、越界由单一 scoped 根统一收口，理论上覆盖更完整。
- 因此**当前记录为"倾向 B，但需 H0 headed evidence 落地后复核"**；若后续 gate
  产出证据显示方案 B 也无法在 WKWebView 中处理特定编码/符号链接形态，再回到
  A/B 混合（B 为主 + 有限 rewrite 兜底）。
- 本文件不包含任何实现。

## 4. 状态机说明

- `H0 = BLOCKED` → 唯一允许的下一状态是 **H1（headed evidence gate）**：提供完整
  build 预算与 GUI 会话，运行真实 Tauri window，用本 fixture 抓取 WebKit 网络面板
  证据后，重新裁定 H0=PASS/FAIL。
- 禁止：在无 headed evidence 时宣称 PASS，或以单元测试/静态走读冒充 headed
  evidence。
