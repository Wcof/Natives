# Natives × DeepSeek Harness 插件（第三业务入口）

ADR-0048：四个**独立** DSH 插件把 Natives 的空间、文件、Token 统计、投资接入
DeepSeek Harness（DSH）主面板。每个插件只有 1 个 `sidebar.panellist` 入口和
1 个同名 `main` key；点击后在主面板以签名嵌入的全屏 iframe 打开对应页面，
不切会话、不开右侧标签、不出现二级菜单或模块聚合页。四个插件可单独安装／
停用／卸载，共用同一份 Core（8770）与同一份 `~/.natives` 数据；现有 Chrome
扩展、本机 Web、Launcher 三个入口行为不变。

```text
integrations/dsh/
├─ runtime/index.js            # 唯一运行时源：解析 Core、schema 护栏、payload 校验、mint URL
├─ runtime/index.test.mjs      # npm run dsh:check
├─ {space,files,tokenusage,fund}/
│  ├─ index.js                 # Host：加载 runtime.js，注册 /natives/<m>/url
│  ├─ runtime.js               # 由 scripts/dsh-sync-runtime.mjs 生成的逐字节副本（勿手改）
│  ├─ client.js                # Client：1 入口 + 主面板全屏 iframe
│  ├─ package.json / cordis.patch.yml / locale/{zh,en}.json / icon.svg / README.md
│  └─ payload/                 # 可选：完整签名产品源快照（scripts/dsh-payload.mjs 生成）
└─ dist/                       # npm pack 产物（不提交）
```

## Core 解析顺序（运行时，ADR-0048 D2）

1. `NATIVES_CORE_BIN`（测试／高级覆盖）；
2. 源码 checkout 的 `target/debug|release/native-file-host`（仅当插件位于
   checkout 内；同时指向同目录 `model-host`）；
3. 本机已安装产品源（`NATIVES_DEV_PRODUCT_SOURCE` → `~/.natives-local/product-source`
   → `/Library/Application Support/Natives`）；
4. 随包 `payload/`（完整签名产品源快照；启动前按 `payload-manifest.json`
   逐文件 sha256 校验，失败拒绝启动）。

不要给运行时强塞 `NATIVES_STATE_DIR`：debug Core 用 `~/.natives-local`，
release Core 用 `~/.natives`，只有显式 override 才透传。

## 交付物与安装

交付物是四个自包含 npm tarball（`npm pack`，每个含 8 个功能文件 + README，
可选 payload）：

```bash
# 一条命令打包（推荐）
npm run dsh:pack                              # 四个无 payload tarball → integrations/dsh/dist/
npm run dsh:pack -- --source <产品源>          # 先密封随包 payload 再打包

# 仅密封 payload（需要一份完整签名产品源；见下方「payload 与签名边界」）
node scripts/dsh-payload.mjs --source <complete-product-source>
```

防呆：不带 `--source` 且插件目录残留 `payload/` 时，`dsh:pack` 直接拒绝打包——
开发信任根 payload 不得被静默分发出去。

对方安装（DSH 插件机制，无 quarantine、无需 Developer ID）：

```bash
install_bundle target=/绝对路径/@natives-dsh-space-0.0.1.tgz
# 或开发迭代用目录链接
install_bundle target=/绝对路径/Natives/integrations/dsh/space
remove_bundle target=@natives/dsh-space
```

四个 tarball 互相独立，装任意子集都可以；卸载不删除用户数据。

### 装载粒度：三层，不要混

| 层 | 由谁决定 | 怎么改 |
| --- | --- | --- |
| 装不装 | profile 的 `dsh.profile.bundles` + `dependencies` | `install_bundle`／`remove_bundle`；不要手改 profile 的 `cordis.yml`（patch 层会覆盖它） |
| 启不启 | Cordis 行 `id`（`natives-space`／`natives-files`／`natives-tokenusage`／`natives-fund`） | 宿主 `setPluginEnabled`；patch 层按 `id` 覆盖 config |
| 数据 | 不随启停变化 | 四包共用同一 Core／8770／`~/.natives`；卸载不删用户数据 |

装的是 bundle，启停的是 entry。一个 bundle 只有一条 entry，所以「只装载其中一个模块」
= 只 `install_bundle` 那一个包，其余三个不装即可，不存在聚合入口或二级菜单。

### DSH 版本

四个 `package.json` 声明 `engines.dsh: ">=0.2.1-rc.3 <0.3.0"`；宿主用
`semver.satisfies(运行时版本, range, { includePrerelease: true })` 判定，缺省视为任意版本。
已实测端到端的是 0.2.1-rc.3（2026-10-29）；本机已升级到 0.2.1-rc.5，**四入口在 rc.5 的回归
尚未执行**，因此该范围是按既有实测下限声明，不是 rc.5 的实测结论。DSH 升 rc 后需重走
下方「安装后验证清单」。

### payload 与签名边界

- **无 payload** 的 tarball：适合已安装 Natives 的机器——插件 attach 已装产品
  的 Core，读写同一份数据。
- **带 payload** 的 tarball：适合没有 Natives 的机器。payload 必须来自**生产
  签名**产品源（release 构建恒定拒绝 `APP_DEV_TRUST_ROOT_IN_PRODUCTION`）；
  开发信任根（`scripts/apps/keys/product-trust-dev`）签出的产品源只能本机
  联调，**不得对外分发**。生产签名产品源就绪前，对外分发保持 payload 缺省
  形态，不冒充。

## 安装后验证清单

1. DSH 侧栏出现对应入口（空间／文件／Token 统计／投资），四个 `main` key 注册。
2. 点击入口：主面板出现全屏页面，**没有** Natives 的产品级导航（空间页无
   文档快捷方式／应用菜单，文件页无「个人空间」链接，app 页无应用菜单与页头）；
   不切换当前会话。
3. 明暗主题跟随 DSH（`?theme=` 只覆盖本次渲染，不写回 `natives-theme`）。
4. 功能抽检：Token 统计出核心统计行与明细表；投资看板加载真实数据；文件页
   能浏览目录；空间页工作区可切换。
5. `remove_bundle` 单个插件后其余三个不受影响；重装后数据仍在。
6. 数据 schema 高于插件运行时 → 入口报「请先升级插件」，不降级写。

## 常用命令

```bash
node scripts/dsh-sync-runtime.mjs          # 改 runtime/index.js 后同步四副本
node scripts/dsh-sync-runtime.mjs --check  # 副本漂移校验
npm run dsh:check                          # 副本一致 + 运行时测试
```

## 纪律

- 改运行时只改 `runtime/index.js`，然后 sync；`runtime.js` 是生成副本。
- 客户端必须用 `window.__ModuleLoader__.load({ id, factory(require) })`，
  React 从 `require('react')` 取；`main` 非 `conversation` key 无 Session 绑定，
  不能用 `commands.execute`。
- 禁止：反向代理 Natives、剥 `X-Frame-Options`、把页面并入 DSH 同源、插件自建
  `<webview>`（主进程拒绝无租约挂载）。

决策与历史：[ADR-0048](../../docs/adr/0048-dsh-plugin-third-entry.md)、
[交接上下文](../../docs/development/dsh-plugin-handoff-2026-10-28.md)。
