# ADR-0029：统一套件安装、官方子应用预装与分级验收

- 状态：accepted-target；2026-09-11 用户要求将本轮主从关系与整改规则落库。表示目标决策生效，不表示安装包或功能已实现。
- **2026-09-13 可见主入口修订（用户明确要求）**：完整 `.pkg` 必须安装唯一 `/Applications/Natives.app`，具备正常应用名称、图标和系统应用登记，用户能在 Finder“应用程序”找到并双击。首次打开自动打开 Chrome，显示有界安装/启用引导并定位随包扩展目录；已有扩展通过本次真实握手后进入 Natives。此为主产品 Launcher 的精确例外，取代此前对主产品也禁止 `.app`/LaunchServices 的表述；基金等内置模块仍不得创建独立 `.app`、系统图标或安装对象。Launcher 只承载打开、首次引导和简短诊断，交接成功或用户关闭即退出，不恢复业务桌面 UI、常驻 Runtime 或第二套安装引擎。普通 Chrome 无法按本地分发路线静默安装扩展时，必须给出真实人工步骤，不能伪报自动成功。
- **离线 HTML 引导补充（2026-09-13，用户要求）**：同一版本必须随 Launcher 提供独立于扩展的本地 HTML 指南，首次双击由 Launcher 在 Chrome 打开；安装器结束页通过 Distribution 的 conclusion 展示同源内容的静态摘要。详细步骤只维护一套内容，原生窗口保留必要系统操作、实际连接结果及异常；HTML 不接 Native Bridge、不启动本地服务、不读取业务数据或推断安装成功。引导属于已授权首次配置范围，不构成第二业务 Surface。具体内容、入口和验收见唯一实施方案 §1.3。
- 2026-09-12 路线收敛（用户最终决定，取代本文冲突的"预装套件二次下载"语义）：Natives 是一个完整应用，基金等功能是**内置模块**，全部随 Natives 完整安装包一起安装、更新和修复；没有模块独立下载、安装、更新、卸载或 Release。取消"Suite Seed 离线预装源 + 首次连接 Seed Reconciliation 预装事务"链路——基金代码随包构建并进入完整安装包，首用只做数据初始化，无任何模块代码下载；取消"用户在应用中心确认添加未随附应用"；取消"兼容子应用独立更新不重发 Core/扩展"——更新统一为"更新 Natives"完整产品版本。数据、Keychain、偏好、移除选择保留规则继续有效。凡与本次收敛冲突的表述以本注记为准。
- **2026-09-17 开发环境产品源布局修订（用户要求）**：root-owned 系统源 `/Library/Application Support/Natives/`（本地开发为 `Natives-Local/`）是**发布/安装形态**的要求，不是开发迭代的要求。本地开发日常迭代（修改内置模块 UI、重建 `natives-app-runtime`、重算载荷 SHA 与重签 dev 产品清单）必须全程在用户可写目录完成（dev 信任根下的 `~/.natives-local/` 命名空间，产品源默认 `~/.natives-local/product-source/`，可用 `NATIVES_DEV_PRODUCT_SOURCE` 等环境变量显式覆盖），**不要求 sudo**。只有进入安装候选实测（A-Local/B-Local 的安装器验收）才构建并写入 root-owned 系统源。dev 工具链（`dev.mjs`、`refresh-dev-product-source.mjs` 等）不得把 sudo 作为日常迭代的前置步骤。
- **2026-09-14 统一内置应用 Runtime 架构修订（ADR-0031）**：[ADR-0031](0031-unified-builtin-app-runtime-and-monorepo-modules.md) 决定所有内置应用编译进统一的产品级 `natives-app-runtime` executable，废除每模块独立 executable（如 fund-host）与独立 Native Host 注册。通过 Single Runtime Binary / Multi Process Instance 机制在保留进程级完全隔离与彻底释放的同时，消除多可执行文件膨胀。
- 取代范围：补充 ADR-0027 的独立交付；取代“首批官方子应用必须由用户逐包获取”以及“本机开发进入基金阶段前必须完成正式签名/公证”的执行含义。ADR-0027 的运行隔离、单一安装权威和数据保护继续有效。部分运行模型细节被 ADR-0031 升级。
- 规范：[子应用标准](../standards/technical/06-sub-apps.md)、[安全标准 R-S14](../standards/technical/02-security.md)、[接入契约](../contracts/managed-app-contract.md)。
- 唯一执行入口：[两阶段整改方案](../development/app-center-fund-implementation-plan.md)。

## 背景与选择

用户需要一个 Natives 主产品，安装后即可在产品内打开基金等随附子应用。此前将“独立发布”直接体现为每个子应用分别下载本机程序，造成安装与系统安全提示混淆。仅把文件改名，或只安装 Core Host，并不能使没有交付的业务代码出现，也不能令子程序自动继承平台信任。

选择一个统一套件安装包（One Product + One Installer），预置明确列出的官方内置模块；各内置模块源码可独立构建与版本化，但**交付与更新只随 Natives 完整产品版本进行**（2026-09-12 收敛，取消运行期独立更新）。业务呈现继续使用现有 Chrome/Chromium 扩展；桌面只允许主产品薄 Launcher 的首次引导/诊断窗口，不新建业务桌面 UI、**禁止把基金业务编译进 native-file-host / Core Files Host（必须明确 Core != App Runtime；允许且要求 Fund 作为 Built-in App Module 编译进专用 `natives-app-runtime`，见 ADR-0031）**，不引入 WASM 解释器或通用 Plugin Runtime。

不选择：仅打包主 Host（不含基金）；所有业务编译进 Core（破坏隔离和独立更新）；为动态业务新建通用运行时（扩大范围）；通过移除 quarantine/关闭 Gatekeeper 实现“无感”；创建独立 Fund.app 或系统级后台常驻。

## 决策

1. **一次产品安装（One Product, One Installer）**：完整 Natives 安装包含主程序薄入口、Core Files Host、既有 Model Host、Chrome 界面组件及基金等全部内置模块代码（首个完整候选含真实 fund 业务与 UI）。不存在"未随附模块"，不保留二次下载或预装源链路（2026-09-12 收敛）。
2. **内置模块即开即用**：安装文件随产品包交付后，首次打开模块只做必要的数据初始化/迁移；不下载代码、不显示"安装基金"、没有预装事务界面语义。功能可用性以实际文件/身份/协议与必要只读检查为准；失败显示修复，不伪造可用。
3. **受控运行与内置模块（Managed Runtime Instances）**：产品关系上，Natives 是唯一产品，内置模块是其组成部分；安装关系上，一次产品安装交付全部组件；Runtime 关系上，模块运行由统一产品级 `natives-app-runtime` 的按需独立进程承载（ADR-0031，Single Runtime Binary / Multi Process Instance），以保留业务进程隔离。独立 Runtime 进程属于 Natives 内部受控运行载荷，绝不意味着独立产品、独立 `.app`、独立用户安装对象、独立 executable 或独立更新节奏（2026-09-12 收敛，取消运行期独立更新；2026-09-14 ADR-0031 收敛为单一 Runtime 二进制）。
4. **只有一个安装权威**：产品安装/更新进入既有 App Store 验签、激活、锁和恢复实现，用于核验完整产品组合内的固定模块文件与身份；不再维护 Suite Seed 预装链路与在线 Catalog 分发（Local Registry 优先级保留为历史表述）。套件/组合清单只声明交付内容，不是第二 App Registry；安装器不得直接写用户 DB/activation、执行基金迁移或以 root 运行子应用。
5. **目录的精确例外**：主产品可见 Launcher 固定在 `/Applications/Natives.app`，同一完整安装引擎负责其安装、更新、签名校验和修复；只包含入口程序、Info.plist、图标及引导资源。受限、root-owned 的 `/Library/Application Support/Natives/` 存主 Host、固定解压扩展目录（`ChromeExtension/`，含 manifest.json）与全部固定内置模块文件，不保留另一份 Launcher 或离线预装源，不存用户数据库、activation 或业务数据。每用户活动模块载荷和业务数据仍在 `~/.natives/`，最小 Native Messaging 注册在浏览器规定位置。只有主产品允许正常 LaunchServices 登记与运行时 Dock 图标，不擅自固定 Dock；模块仍禁止写 `/Applications`、创建独立 `.app`/系统图标。正式与本地开发入口身份及路径隔离，见契约 §3.2/§4.2。**（2026-09-17 修订）root-owned 系统源是发布/安装候选形态的要求：本地开发日常迭代不写 root 目录，产品源使用 dev 信任根下用户可写的 `~/.natives-local/product-source/`（环境变量可覆盖），验签、载荷 hash/摘要核验规则不变；仅安装候选实测构建 root 系统源。**
6. **组合清单固定（2026-09-12 收敛）**：本期包含 fund，不意味着所有未来应用预先存在；新增内置模块只能通过新的完整 Natives 版本交付。取消"用户在应用中心确认添加未随附应用"与"兼容子应用独立更新不重发 Core/扩展"；不默认开启静默自动更新，产品更新由用户触发的"更新 Natives"完成。
7. **尊重用户选择**：产品重装/升级必须保留模块显示/启用/排序偏好与用户数据；历史停用/移除记录转为入口关闭偏好，整包更新不自动打开（2026-09-12 收敛：不存在 Seed 版本比较或"显式恢复才重装代码"的语义——模块代码随完整产品交付，开启入口只改偏好）。组合清单删去某模块也不自动删用户数据。
8. **本地与正式不同信任策略**：本地模式允许本机构建的 ad-hoc/开发签名载荷，无需 Developer ID/Apple 公证。必须同时满足专用非生产构建身份、显式开发模式、隔离的安装/注册/数据/Keychain 命名空间、精确本机构建摘要核验。开发组合清单仍验签、载荷仍验 hash/格式/权限。仅有 fixture=true、环境变量或“我在本地”标签不构成豁免。本地开发禁止关闭 Gatekeeper。
9. **正式安全要求不变**：生产二进制拒绝开发信任根/fixture；正式套件及其子程序必须完成相应平台的签名、身份与公证验证。安装器被信任不代表子程序自动可信，`.pkg` 或 ad-hoc 签名不能保证没有系统提示。任何模式都不得通过清除 quarantine、全局关闭 Gatekeeper 或未审查安装脚本绕过安全控制。

## 两阶段与验收语义

- 阶段 A 保留 A0—A5；2026-09-12 收敛后取消"先用无基金逻辑的独立标准样例验证再进入基金"的前置条件，改用真实内置基金验证完整产品组合的真实浏览器、Native Host、启停、整包更新、恢复与数据保护；已有独立样例仅作为覆盖共享底层规则的低层测试 fixture 保留。
- **A-Local**：所选本机平台上的全部工程用例通过，包括本地模式正反向安全测试、真实浏览器/进程/锁、数据保护、安装候选实测及有可比证据的性能检查。没有签名凭据不能使这些用例自动通过。
- A-Local 通过才进入 B0—B3。**B-Local** 同样要求真实基金业务、真实数据来源、迁移和统一套件可用；未通过不得称“本机可用完成”。
- 原 A-G1—A-G10/B-G1—B-G6 的验收项目全部保留；证据分列 local/production。正式平台签名、公证、生产信任根、发布浏览器身份/审核及声明平台验收组成 **Release Gate**。A-Local/B-Local 不等于原完整 A-Gate/B-Gate。
- 此处明确调整旧的单一阶段门槛，允许本地工程先闭环、正式交付条件后完成；没有取消任何数据、安全或正式发布检查。外部条件 pending 不得伪造 passed，也不必阻塞与其无关的本地实现。
- 外部公开 Release/Catalog、给其他用户推送更新均另行取得授权。本轮只批准规范、方案及其明确的本地候选目标，不授权安装到用户日常环境或清除个人数据。

## 后果与迁移

安装包体积增加，但 Core 二进制和扩展业务体积预算不增加；内置模块不运行时必须无进程/端口/业务计时器。资源报告分别列主 Host、每个内置模块、完整产品包，不用新的总包掩盖回归。

旧 `installers/macos/build-pkg.sh` 只有 Files Host，并创建旧 Workbench.app。只复用 pkgbuild/productbuild 等工具与安全资产，按单一安装引擎改造（一个引擎、一份签名产品组合清单），不直接执行旧脚本冒充完整产品。现有 apps:dev 是开发测试入口，不替代统一安装包验收。2026-09-12 收敛后不再维护离线预装源（Suite Seed）与 Seed Reconciliation 链路；引用该链路的 Standards/契约条目按实施方案 P0 同步修订。

同步范围：ADR-0020/0027 取代注记、Standards product/01 与 technical/01/02/03/06、AGENTS、契约、文档任务入口和唯一实施方案。现有术语写入仓库既有 glossary，不另建第二份领域词典。runtimeType 模型与 R-SUBAPP-NATIVE-01 由 06-sub-apps 定义；契约 manifest 字段调整由 managed-app-contract 承载。

## 平台依据

- [Chrome Native Messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)：浏览器启动 Host、注册位置与调用 origin；多浏览器路径须逐一核验。
- [Apple 本地代码签名](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements)：ad-hoc 是本地运行签名，跨构建身份不稳定。
- [Apple 安全打开软件](https://support.apple.com/zh-cn/102445)：系统提示、Developer ID 与公证不是产品归属的判断。
