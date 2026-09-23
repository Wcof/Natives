# 技术 04 · 性能与资源生命周期

> 版本：4.0.0 · 日期：2026-09-14
> 当前整改与证据记录：[`../../architecture/application-performance-remediation.md`](../../architecture/application-performance-remediation.md)

## 统一预算

| 指标 | 预算 |
|---|---:|
| 冷启动到可交互 p75 | ≤ 2.5 s |
| 普通交互 p95 | ≤ 100 ms |
| 已缓存页面切换 | ≤ 300 ms |
| 主线程单任务 | ≤ 50 ms |
| 可见动画 | ≥ 55 FPS |
| 单入口初始 JS/CSS gzip | ≤ 350 KiB |
| Extension 分发体积估算 | ≤ 360 KiB |
| 空闲 60 s 总 CPU | ≤ 2% |
| 30 分钟循环空闲回落后 RSS 增长 | ≤ 15% |
| Files Host Release RSS | ≤ 12 MiB |
| Files Host 空闲 CPU | ≤ 0.5% |
| Files/App Host EOF 退出 | ≤ 2 s |
| 10k 目录首 100 项查询 p95 | ≤ 50 ms |
| Model Host 首快照冷启动 p75 | ≤ 2.5 s |
| Model Host 热快照 p95 | ≤ 300 ms |
| Model Host 空闲 RSS | ≤ 256 MiB |
| Model Host 空闲 CPU | ≤ 5% |
| Model Host EOF 退出 | ≤ 1 s |
| 每 appId 业务实例 | ≤ 1 |
| 同用户活动 App Host | ≤ 4 |

#### R-P1 · 性能结论必须可比较

- **等级**：MUST
- 同设备、系统/Chrome 版本、构建类型、数据集和操作路径记录 before/after。
- 报告原始样本、p50/p75/p95、RSS/CPU、进程数和失败项；单次通过不能证明稳定。
- Release 结论必须来自 Release 构建；本地 debug 只用于定位。

#### R-P2 · 页面主线程不做重 IO

- **等级**：MUST
- 文件、SQLite、进程、工具日志、Provider 网络和大数据聚合只在所属 Host。
- 页面解析/渲染预计超过 16 ms 的工作应切片、延迟或窗口化。
- pointer move 只改草稿内存；stop/flush 最多一次持久化。

#### R-P3 · 副作用必须可释放

- **等级**：MUST
- 每个 event listener、timer、observer、Native Port、object URL 和异步 owner 都有明确 disposer。
- 页面/Widget 重绘前先 dispose 旧实例；页面隐藏时暂停非必要工作。
- 测试必须覆盖 destroy、pagehide、BFCache restore 和异常 disconnect。

#### R-P4 · 页面级去重

- **等级**：MUST
- 相同领域 query、时间 tick、可见性监听和刷新事件按页面共享。
- Widget 数量增加不得线性增加 Host 查询、SQLite 查询、Native Port 或永久 timer。
- 用户启动的倒计时按绝对目标时间恢复，隐藏不造成时间漂移。

#### R-P5 · 数据与 DOM 有界

- **等级**：MUST
- 超过 200 项的 UI 使用分页、窗口或虚拟化；缓存、Map、Set、队列和日志有容量与失效条件。
- Usage 必须验证 10k/100k/500k 事件；Files 必须验证 10k/100k 目录项。
- 请求取消或调用方销毁后，不得长期保留 DOM、配置或响应缓冲。

#### R-P6 · 事件优先且隐藏门控

- **等级**：MUST
- 实时状态优先 Host event；轮询必须有真实必要性、可见性门控和下限周期。
- 纯时间 UI 最高 1 Hz；后台轮播、刷新和同步在 hidden 时停止。
- Service Worker 禁止轮询、Native Port 和 keepalive。

#### R-P7 · 按需加载与按需进程

- **等级**：MUST
- 未打开入口不进入其初始依赖图；重型图表、预览和设置视图按需加载。
- Files/App Host 未打开时进程数为 0。
- Model Host 默认由页面连接拥有；resident 仅用户显式开启，最多一个 worker。

#### R-P8 · Files 生命周期

- **等级**：MUST
- 文件页未打开时无 Host；hidden 且无任务 60 秒断开。
- EOF 后取消 Watch、导入和后台任务并等待回收。
- Release RSS、CPU、目录查询和退出满足统一预算。

#### R-P9 · 内置模块生命周期与进程级资源回收

- **等级**：MUST
- 未运行模块零内存（≈0 dedicated runtime memory）：未被打开激活的模块不分配堆内存、不打开 SQLite 数据库、不创建 HTTP 路由、不启动 Timer/Worker。
- 隐藏 60 秒、页面关闭或 Native Port 断开（stdin EOF）后进入统一 shutdown：停止模块、移除 iframe、撤销 token、断开 Port。
- **硬 Gate：Native Port 断开（stdin EOF）到 App Runtime Process 退出时间 $\le 2$ 秒**。
- 进程退出后由操作系统彻底回收全部内存与句柄：Module Process = 0, Listener = 0, Timer = 0, Worker Thread = 0, SQLite Connection = 0, Network Client = 0。
- 冷启动 p75 ≤ 2.5 s；反复打开/关闭循环测试（至少 20 次）无孤儿进程、无端口泄漏、无监听残留、无锁泄漏。
- 新 Fund Ready RSS 不得比旧 Fund Ready RSS 回归 > 10%。

#### R-P10 · Model Host 生命周期

- **等级**：MUST
- 配置快照不等待无关 Usage、价格、导入或全部 Secret 预热。
- Usage/Proxy 初始化可以延迟，但首次相关请求必须完整、单次且错误诚实。
- resident=false 时最后连接关闭后退出；resident=true 时状态可见、单实例、可关闭。

#### R-P11 · 动画与视觉成本

- **等级**：MUST
- 装饰动画优先 transform/opacity，不为每个组件运行永久 rAF。
- 支持 `prefers-reduced-motion`；持续 spinner 在减弱动效下静止或替换。
- blur/filter/shadow 不得让滚动、拖拽或低性能设备超预算。

#### R-P12 · 性能门禁不能自欺

- **等级**：MUST
- 门禁必须构建或验证当前源码对应的二进制，不能只比较单个源文件 mtime。
- 不支持的 CPU/RSS/平台项不得以 PASS 混入正式结论。
- 不得调大预算、删除样本或启用常驻来掩盖回归。

## 验收场景

真实 Chrome 先预热 5 分钟，再运行 30 分钟：循环 Home/Files/Apps/Model 设置、20 个 Widget、
7 个 AI 效能卡片、5 个时钟卡片、基金打开/隐藏/关闭。至少每 10 秒采集 Renderer 和各 Host
的 RSS、CPU、进程、Port、timer/listener。结束后空闲 60 秒，按统一预算判定。

## 合规自检

- [ ] before/after 条件一致，Release 样本不少于 5 个。
- [ ] 每个副作用有 owner 和 disposer。
- [ ] hidden 无不必要唤醒，非驻留资源归零。
- [ ] 长数据、DOM、缓存和并发有界。
- [ ] 所有产品入口单独计算首屏依赖。
- [ ] 30 分钟增长 ≤15%，不支持项明确阻断或标记。
