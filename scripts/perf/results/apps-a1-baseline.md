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
