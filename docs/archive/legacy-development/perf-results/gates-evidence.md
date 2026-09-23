# Final Gates Evidence — Preview Capability V2 (T70/T80)

> 分支: deploy（T80 convergence 完成）
> HEAD: 6f28f9f7（本地 deploy 分支；deploy@91d997e 祖先链已确认）
> 全部命令经 rtk 执行，exit code 如实记录；本文件为权威证据汇总（2026-08-08 复核）。

## 7 项 Gates

| Gate | exit | 结果 |
|---|---|---|
| rtk npm run typecheck | 0 | tsc --noEmit |
| rtk npm run lint | 0 | i18n 2559 zh = 2559 en；无新增硬编码色 |
| rtk npm run test | 0 | 859 / 859 pass |
| rtk npm run perf:check | 0 | /files 239.6KB gzip ≤ 350KB |
| rtk cargo fmt --check | 0 | clean |
| rtk cargo test --workspace | 0 | 1869 passed, 16 ignored (47 suites, 123.22s) |
| rtk npm run protocol:check | 0 | TS aligned；Rust methods 156 |

## H0 HTML Strategy Gate

- 三态结论：**BLOCKED**（本机无真实 Tauri/WebKit headed evidence，不 fake PASS）。
- fixture 与已执行证据：`spikes/html-preview-case/`（root 外 secret `TOP-SECRET-OUTSIDE`、
  symlink mode 120000 → `../outside-secret.txt`、encoded traversal、合法 `../`、CJK 空格路径）；
  详见 `docs/h0-decision.md` §2.2。
- 只冻结 HTML lane（T20/T21/T15），不阻塞其它 lane。

## Perf before/after

同设备/同构建/同数据（seed=20260808）：BEFORE=deploy@91d997e，AFTER=本 head。
详见 `scripts/perf/results/before-after-evidence.md`：
50k 条目 DOM 恒为窗口 1603 节点（O(viewport)）、click p95 0.001ms（预算 ≤100ms）、
20×1ms 阻塞 Host IO 25ms→1.2ms（async + bounded semaphore + spawn_blocking）。

## 备注

cargo test --workspace 存在既有 sidecar_supervisor 并行抖动（进程生命周期测试时序敏感；
该文件本分支无 diff，隔离运行 1/1 通过）；重跑即全绿，非本轮引入回归。
