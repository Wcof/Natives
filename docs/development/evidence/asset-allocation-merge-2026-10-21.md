# 首页资产配置合并验收

用户确认的合并卡片 Demo 仅作视觉参考。生产页面将资产分布与组合设置合成一张“资产配置”卡片，共享分类管理、目标编辑、分类明细与三项摘要；左右实际／目标对照，窄窗上下排列。数据继续读取 Fund `groupAllocation`，分类和目标沿用 group-plan writer，无 schema 或真实用户数据变更。

## 实现与验收对应

| 需求 | 实现 | 验证 |
| --- | --- | --- |
| 一张卡片、三项摘要、实际与目标同屏 | `ui/src/partials/view-home.html`、`home-overview.js`、`styles/home.css`、`styles/investment.css` | 单一卡片、精确归类金额／目标合计／待归类、分类树与语义表格 |
| 继续保存和查看分类／目标 | 既有 `home.manageGroupPlan`，三个原生按钮入口 | 真实隔离账本通过表单绑定两项资产；70／30 保存后得到真实 80／20 分布，修改 65／35 并重开／刷新保持 |
| 隐私、缺数据与安全文本 | 三态投影、未知总额不补零、类别文本转义、图最多六个系列 | 合成传输回归覆盖 hidden／percent、空配置、缺估值、八类聚合、恶意名称、展开／焦点／滚动保留 |
| 主题、语言、窄屏和键盘 | 现有语义 tokens 与中英文 locale；窄窗摘要单列、目标表分行 | Archive／Volt × zh-CN／en × 1280／320 px；真实宿主窄窗实际卡片宽 207 px，无卡片／表格横向溢出；Enter 打开、Escape 关闭并恢复焦点 |

源码修改后通过 `modules:ui:build` 生成 dist；`modules/fund/src/ui.rs` 已包含上述 CSS／HTML／JS，不新增资源入口。中文“管理分类／编辑目标”及英文同步，模块目录内的 i18n 由单源构建生成。

## 实际结果

- `rtk npm run modules:ui:build`：通过。
- `rtk npm run modules:ui:check`：通过；最终 `perf:check` 另包含该项复验。
- `rtk proxy node scripts/modules/asset-allocation-browser.test.mjs`：8 组真实 Chrome 渲染与交互检查通过；此脚本使用明确标记的合成 HTTP 传输，不冒充真实账本。
- 真实 Chrome → 认证本机 Web → 受限 sandbox → Core／签名 Runtime → 隔离 Fund SQLite：保存、修改、详情重开、刷新持久化及 8 组显示通过，pageerror 为空。两个登记资产的估值为 8,000.00 和 2,000.00，归类合计 10,000.00，真实分布 80.0%／20.0%，与保存目标的偏差一致。
- 最终窄窗截图已实际检查，金额和名称可读；两栏正文继续使用既有 480 px 预算和滚动 owner。项目临时原始结果在 `.tmp/asset-allocation-merge/run-yQf6v7/report.json`；含截图与隔离数据，正式产品不依赖这些文件。首轮临时验证脚本错误使用 storage 导出，修正为现有 `storage.local.set` 后通过，未修改生产 storage。
- 末次样式细化后的最终串行 `rtk npm run perf:check` 通过（含 Extension／Files／Apps／模块 UI，Files p95 38.885 ms）；`standards:check`、`git diff --check` 通过。原始日志 `.tmp/asset-allocation-merge/perf-final.log`。

## 范围与限制

未修改账本、估值和分类保存协议，无新依赖、定时器、订阅或进程。未读取真实投资库、配置 AI、改真实凭据，未提交、推送、安装或发布。正在运行的旧开发产品源／Worker 未替换；本次源码与生成资源需由既有开发启动／密封流程加载。没有重跑完整 30 分钟长测，也未进行 Windows 或人工读屏验收。

## 2026-10-21 小条语义与滚动条留白修正

用户指出填色不符合当前实际占比、右侧内容可能被滚动条覆盖。复现：真实 UI 的两项实际均为 50.0%，原条形宽度却分别是目标的 60%／40%；新增断言在修改前失败。现统一用 `currentTenths` 绘制，以 100% 为总长度；空白为剩余比例，移除目标填色和细线；未知实际不画条，零值仍保留 0% 空条，hidden 移除图形。中英文说明同步解释颜色含义。

首页外层保留稳定滚动条槽位，分类／目标与资讯／AI 正文右侧至少留 12 px。Chrome 回归额外强制 16 px 滚动条，测量目标表实际边界，确认表格结束位置在滚动条与 12 px 内容间距之前。继续覆盖 8 组主题／语言／宽度、0%／100%／未知、隐私、展开／焦点和滚动。

真实隔离账本复验通过：80%／20% 的实际分布绘制 80%／20% 填色，70%／30% 目标不再决定条形宽度；保存、修改、重开、刷新及 8 组 Chrome 布局通过，pageerror 为空。原始报告 `.tmp/asset-allocation-merge/run-9fTbps/report.json`，截图已实际检查。最终串行 `perf:check` 通过（含模块 UI／Extension／Files／Apps），日志为 `.tmp/asset-allocation-merge/perf-progress-gap.log`；standards 与 diff 检查通过，结果同步 ROADMAP。本次只影响 UI 投影与间距，真实开发源／旧 Worker 未替换，未改用户投资数据。
