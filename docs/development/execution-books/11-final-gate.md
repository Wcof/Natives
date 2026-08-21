# 11｜Final Integration & Test Gate V2

## A Architecture
Workspace/Theme Source of Truth 均唯一；无旧 Home/Agent/Assistant/Harness/V1 Theme runtime authority。

## B Build
`npm run typecheck` / `npm run lint` / `npm test` / `npm run build` / `npm run perf:bundle` / 最终 Rust workspace cargo test / Tauri build。

## C Theme Contract
dark/light semantic key parity：canvas/surface/material/edge/highlight/shadow/text/control/chart/motion/overlay。

## D Dark Glow
暗环境有层级；glow 只用于 focus/selected/data/status；密集内容不过度透明；无霓虹墙。

## E Liquid Crystal
必须全部 PASS：
1. Specular Highlight；
2. 多层微阴影；
3. 石墨灰/中性灰文字层级；
4. 图表约 15%→0% area fill；
5. 晶透用于合适层级，正文稳定；
6. 不是 dark 机械反色。

## F 全路由视觉
`/ /ai /apps /capabilities /files /jobs /library /modules /store /tools /usage` + Settings + CommandPalette + Notification + RightPanel + Modal/Toast/Tooltip。
全部 dark/light。

## G Interaction
compact grid、free canvas、drag/resize/pan/zoom、inspector、workspace tabs/session。

## H Performance/A11y
无全屏高成本 blur/WebGL；drag 可降级；reduced motion/transparency；125%/150%；窄窗口；键盘焦点。

## I Legacy Death
新写入旧主题名=0；V1 theme authority=0；无用 liquid-glass-react=0；旧 Agent runtime=0；旧 Home 双写=0；参考源码复制=0。

## J 最终报告
逐 Gate PASS/FAIL、证据、Owner、修复、重跑结果；P0/P1 未通过不得宣布完成。
