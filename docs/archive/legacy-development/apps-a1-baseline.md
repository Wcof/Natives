# Apps Framework A1 Baseline（Phase A1 Gate 证据）

> 分支: Deploying
> HEAD: 52b43104
> 设备: Apple Silicon / macOS / Release
> 日期: 2026-09-06
> 用途: ADR-0025 D4「Apps Framework 增量 Gate（≤20 KiB，相对本 baseline）」与 D5「Bundle Recovery 目标 ≤270 KiB」的参照基线。

## 1. Extension Bundle

命令: `node scripts/perf/check-extension-bundle.mjs`

| 项 | 值 |
|---|---:|
| budget（Hard Gate） | 307,200 bytes（300 KiB） |
| estimate（gzip 估算） | **283,245 bytes（≈276.6 KiB）** |
| rawBytes | 953,795 |
| headroom | 23,955 bytes（≈23.4 KiB） |

结论: baseline 283,245 bytes 高于 D5 的 270 KiB 目标（276,480 bytes），缺口 6,765 bytes，Phase A1 必须完成 Bundle Recovery 后才进入 A4 App Center UI。

## 2. native-file-host

命令: `node scripts/perf/check-native-host.mjs`

| 项 | 值 | Gate（ADR-0025 D19） |
|---|---:|---|
| Release 二进制 | **3,610,848 bytes（≈3.44 MiB）** | 目标 ≤3 MiB / 硬 Gate 4 MiB（4,194,304） |
| 空闲 RSS | **8,304 KB（≈8.1 MiB）** | ≤ 12 MB |
| EOF 退出 | **6 ms** | ≤ 2 s |
| 本次 App Store 逻辑 binary delta 上限 | — | ≤ 64 KiB（超出必须依赖归因） |

## 3. 其他 Gate 现状

| Gate | 现状 |
|---|---|
| `npm run perf:check`（= perf:files） | 改动前为绿基线；每次 A 阶段改动后重跑 |
| `npm run extension:check` | 改动前为绿基线 |
| `cargo fmt --check` / `cargo test --workspace` | 最终集成时统一跑一次 |

## 4. A 阶段重测纪律

- 每次 A 阶段合入后重跑 `perf:extension` 与 `perf:native-host`，把数值追加到本文件（不删历史行）。
- `deltaFromBaseline = estimate − 283,245`；Apps Framework 累计 delta 超过 20,480 bytes（20 KiB）时 CI 必须失败并给出文件级归因。
- native-file-host 每次改动后核对 `bytes − 3,610,848 ≤ 65,536`。

## 5. 2026-09-08 App Center 集成证据

设备为同一 Apple Silicon/macOS，Node 20.20.2，Native Host 使用 Release 构建；当前工作区基于 `deploy` 的 `a5f7da3a`，含其他已存在的 Model Host 改动。下表不是只归因于 App Center 的性能收益。

| 指标 | 改前 | 集成后 |
|---|---:|---:|
| 扩展估算，同一浏览器资产纳入规则 | 303,256 bytes | 268,553 bytes |
| 扩展原始字节 | 1,030,750 | 753,333 |
| 扩展资产数 | 126 | 129 |
| 300 KiB 预算余量 | 3,944 bytes | 38,647 bytes |
| native-file-host Release | 1,830,112 bytes | 1,896,880 bytes |

比较方法：取 `a5f7da3a` 的浏览器源码与当前分发文件，均纳入 App UI、目录及签名、locale，均不纳入 Native 包；逐文件使用相同 gzip 与 `76 + 文件名长度` 估算。当前分发先由 `scripts/extension-package.mjs` 使用 esbuild 压缩 JS/CSS 与普通 JSON；签名目录的字节不改写。源码压缩是本次实现的一部分。旧估算器漏掉了 `apps/`，因此不能用其输出宣称删除 `.nap` 后就降至约 140 KiB。本节比较与第 1 节历史 A1 快照口径有区别，历史数值保留。

相对历史 A1 的扩展 delta 为 -14,692 bytes，低于 20 KiB 增量限制；当前值低于 270 KiB 的 Bundle Recovery 目标。Core Release 相对本轮改前增加 66,768 bytes，略高于 64 KiB 归因阈值：macOS 新增直接依赖只有 `semver`，用于版本解析与拒绝降级；其余增加来自安装恢复记录、路径/锁检查、健康检查与数据清理。Windows 专用 `windows-sys` 不进入 macOS 二进制。未修改预算或使用豁免。

本轮最终 `rtk npm run perf:check`（Node 20）通过，已包含 `extension:check`、`perf:files` 和 `apps:check`，不重复执行这些子门禁：

- Core Host：RSS 8,320 KB，EOF 10 ms；10,000 文件、5 次 list_dir 的 P95 为 38.49 ms，低于 50 ms。
- Demo 1.1.0：NAP 163,826 bytes，payload 336,208 bytes；真实进程 Native framing 测试验证安装、ping、运行锁、默认保留及显式清理；EOF 为 1 ms。
- `cargo test --workspace`：45 个 Core 测试、85 个 Host 测试全部通过；`cargo fmt --check` 通过。
- `cargo check -p native-file-host -p demo-host --target x86_64-pc-windows-gnu` 通过；这是交叉编译检查，不代表 Windows 实机通过。
- Chrome/Playwright：使用真实分发字节及隔离的 Native 进程桥接，验证下载取消、镜像切换、损坏包拒绝、离线重试、安装、打开、卸载、第二次确认取消与数据删除。1280×900 / 390×844 截图在 `dist/app-browser-evidence/`；无页面脚本异常及内容横向溢出。

浏览器测试明确使用测试桥接，未修改用户 Chrome 的真实 Native 注册项。公共镜像可达性、Windows 实机注册、系统 Keychain/Credential Manager 删除、Core CPU/GPU 和 App 长时间空闲资源仍是发布验收项；现有 Core 性能脚本对 CPU/GPU 返回 `unsupported`，不把它描述成实测通过。

## 6. 发布与阶段边界

实现顺序调整为：先确认 ADR/现状和 Demo 真实调用闭环，再处理目录分发、事务升级、可重试清理与统一打包，最后集成门禁和发布；Fund 仍未开放安装，不把目录占位当成已交付应用。

- 官方目录固定使用 `Wcof/Natives` 的 `app-catalog-v1` Release，避免 Core 的最新 Release 覆盖目录入口；Demo 使用不可变版本标签 `apps-demo-v1.1.0`。镜像只允许编译时固定的 `ghproxy.net`，只在网络错误时换源；验证失败终止。
- `scripts/apps/package-demo.mjs` 从当前平台的 Release 二进制生成 `.nap` 和目录片段；`merge-catalog.mjs` 合并并签名。签名私钥必须匹配扩展编译公钥，不提交私钥。
- `.github/workflows/app-release.yml` 为三平台构建执行真实 Native 集成测试；此测试用 `--fixture-catalog` 使用刚构建的包，不需要生产私钥。目录合并阶段需要 GitHub Secret `NATIVES_CATALOG_SIGNING_KEY`。
- 工作流默认只产出候选文件；显式选择 `publish` 才发布。`publish-release.mjs` 默认仅验证签名、全部资产 hash/size 及 gzip 展开限制；`--publish` 先发布不可变 Runtime 资产，再更新目录和签名。已有版本资产只有字节一致才能复用，不允许覆盖不同内容。
- GitHub 更新两个目录附件不具备跨文件原子性。短暂的新旧字节不一致会验签失败，页面保留重试入口，不能绕过验签。并发发布须由工作流串行化。
- 本地已生成并核验 `dist/app-release/` 候选及 `dist/extension/` 扩展包；未推送或发布。2026-09-08 GitHub Releases 查询返回空列表，内置 Demo/Fund 仍标记 `published: false`，不显示虚假的线上可安装状态。

复跑浏览器验收需可用的 Playwright 模块和 Chrome；可以通过 `NATIVES_PLAYWRIGHT_MODULE` 指定已安装模块的绝对路径，通过 `NATIVES_BROWSER_CHANNEL=chrome` 使用 Chrome，然后运行 `rtk npm run apps:browser`。测试只写隔离临时目录和 `dist/app-browser-evidence/`。

## 7. 2026-09-08 接续核验与真实 Chrome 发布阻塞

前任务因 API 限流失败，未完成真实 Chrome Native Messaging 验收及提交推送。本次接续修复了并发 Host 打开 App Store 时的 `database is locked`：数据库文件旁的 `apps-migration.lock` 串行化首次 WAL 初始化及迁移；迁移、安装开始和卸载记录事务先获取 SQLite 写锁，避免先读后写的快照升级竞争。8 个连接同时打开新数据库、每个重复 10 次的回归在修复前失败、修复后通过。

- `cargo fmt --check`、工作区 133 项测试（Core 45、Host 88）通过；回归移动至现有 `tests/updates.rs` 后单独复跑通过，文件规模门禁保持不变。
- Node 20 下 `perf:check` 通过，包含 `extension:check`、`perf:files`、`apps:check`。Core Release 1,896,880 bytes、RSS 8,144 KB、EOF 8 ms、10,000 文件列表 P95 45.16 ms。
- `apps:integration` 通过，Demo EOF 1 ms；`apps:browser` 隔离 Native 进程桥接流程通过安装、镜像、取消、损坏包、离线重试、打开、卸载和数据清理。
- 新增可调用入口 `apps:chrome-native`，使用 Chrome for Testing、临时 profile 注册、真实 `chrome.runtime.connectNative`。Core 握手通过，但 Demo 健康检查阻塞在 macOS 加载器。`syspolicyd` 日志明确显示 Demo 缺少有效信任规则并等待系统提示响应；`security find-identity -v -p codesigning` 返回 0 个有效身份。延长健康检查至 10 秒仍失败，试验已撤回，未关闭 Gatekeeper 或绕过签名检查。
- 因此真实 Chrome 的 Demo 安装、打开及卸载闭环仍为 **BLOCKED**，不能把测试桥接的成功作为真实注册验收。需要可用的 Developer ID 签名及公证配置，重新生成并签署 NAP/Catalog 后复跑。正式发布仍未执行，内置 `published: false` 保持真实状态；GitHub 尚未配置 Catalog 签名 Secret，跨平台发布作业亦需该配置。

以上更新补充第 5–6 节证据，不改变 Windows 实机、镜像可达性、系统凭证删除等尚未验收的发布边界；M1 不标为全通过，Fund 开发未启动。
