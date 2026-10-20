# Natives 架构整改方案偏差记录（Deviations Log）

本文档按照实施路线图规范，记录架构整改实施过程中遇到的方案与当前真实源码的差异、分析与工程决策。

---

## Deviation 01: ADR 编号调整（ADR-0034 -> ADR-0035）

- **原方案**：审查文档中草拟为 `ADR-0034: 退役 App Runtime 固定 8765，恢复会话级动态端口`。
- **实际源码**：仓库中已存在 `docs/adr/0034-google-public-oauth-client-exemption.md`（Google Antigravity 公共 OAuth client 凭据豁免登记，2026-09-28 已接受）。
- **差异**：编号 0034 已被占用。
- **最终决策**：将退役固定端口决策命名为 `docs/adr/0035-retire-fixed-app-runtime-port.md`（ADR-0035），内容、决策项和架构约束与原草案 100% 相同。
- **原因**：保持既有 ADR 编号的唯一性和历史不可篡改性。
- **影响**：相关标准文档中引用此决策时指向 ADR-0035。
- **验证方式**：`npm run standards:check` 与 ADR 目录一致性。

---

## Deviation 02: 模块 UI 文件已在仓库中存在

- **原方案**：审查包根据 clean zip 假设 `modules/fund/ui/dist` 与 `modules/tokenusage/ui/dist` 无文件（A-11）。
- **实际源码**：真实 Git 工作区中，`modules/fund/ui/dist`（28 个 JS/HTML/CSS 文件，含 ECharts、缠论分析、分时图等）与 `modules/tokenusage/ui/dist`（`app.js`, `index.html`）均完好存在。
- **差异**：不需要重新无中生有写 UI 业务代码，而是需要理清 UI 源码组织结构并建立规范的 UI 校验与构建脚本（Wave 8）。
- **最终决策**：保留现有完整业务 UI，补充统一 UI build 脚本 `npm run modules:ui:build` 与 CI gate `npm run modules:ui:check`。
- **原因**：避免重做已有工作，确保 clean checkout 任意机器均可一键构建 UI dist。
- **影响**：加速实施，重点聚焦架构收口与门禁加固。
- **验证方式**：UI 构建测试与静态门禁。
