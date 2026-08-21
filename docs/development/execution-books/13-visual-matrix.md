# 13｜全 App 视觉迁移矩阵（源码级）

## 路由
- `src/app/ai/page.tsx`
- `src/app/apps/page.tsx`
- `src/app/capabilities/page.tsx`
- `src/app/files/page.tsx`
- `src/app/jobs/page.tsx`
- `src/app/library/page.tsx`
- `src/app/modules/page.tsx`
- `src/app/page.tsx`
- `src/app/store/page.tsx`
- `src/app/tools/page.tsx`
- `src/app/usage/page.tsx`

## 域矩阵
| 域 | 源码范围 | Owner | 核心要求 |
|---|---|---|---|
| Shell | `src/components/shell/**` | B+Main | 根背景、Sidebar、浮层、Terminal 双材质 |
| Home/Workspace | `src/components/home/**` | C | surfacePolicy + 编辑态 chrome |
| Files/Preview | `src/components/files/**; src/components/preview/**` | C | 内容可读性优先 |
| Apps/Creative | `src/components/apps/**; src/components/creative/**` | C | 卡片/详情/安装弹窗统一 |
| Library | `src/components/library/**` | C | 列表/详情/空态统一 |
| Capabilities | `src/components/capabilities/**` | C | 高密度信息层级 |
| AI integration | `src/components/ai/**` | C | 外部 AI 工具 UI，不复活 Agent Runtime |
| Jobs | `src/components/jobs/**` | C | 仅迁移最终保留的后台任务 UI |
| Usage/Dashboard | `src/components/dashboard/**; src/app/usage/**` | B+C | Light area 15%→0% |
| Settings | `src/components/settings/**` | B+C | Appearance + Provider/Proxy 全迁移 |
| UI primitive | `src/components/ui/**` | B | V2 唯一 primitive |
| Edge flows | onboarding/release/update/screenshot | C | 非主路由也不能漏 |

## 组件域规模
| 域 | TS/TSX 数量 |
|---|---:|
| `src/components/shell` | 42 |
| `src/components/assistant` | 41 |
| `src/components/files` | 31 |
| `src/components/settings` | 31 |
| `src/components/capabilities` | 18 |
| `src/components/creative` | 14 |
| `src/components/ui` | 14 |
| `src/components/ai` | 13 |
| `src/components/preview` | 13 |
| `src/components/home` | 9 |
| `src/components/library` | 8 |
| `src/components/dashboard` | 5 |
| `src/components/jobs` | 4 |
| `src/components/menubar` | 3 |
| `src/components/release` | 2 |
| `src/components/screenshot` | 2 |
| `src/components/apps` | 1 |
| `src/components/onboarding` | 1 |
| `src/components/tools` | 1 |
| `src/components/update` | 1 |

## 高风险硬编码文件（粗略命中）
| 文件 | 命中 |
|---|---:|
| `src/app/styles/tokens.css` | 210 |
| `src/lib/file-badges.ts` | 78 |
| `src/lib/theme-engine.ts` | 47 |
| `src/lib/provider-presets.ts` | 25 |
| `src/lib/library-api.ts` | 8 |
| `src/lib/usage-export.ts` | 6 |
| `src/lib/file-icons.tsx` | 4 |
| `src/lib/design-tokens.ts` | 4 |
| `src/lib/shiki-utils.ts` | 3 |
| `src/lib/notification-ui.ts` | 3 |
| `src/lib/iframe-manager.ts` | 3 |

## 签收
- 每个域检查 default/hover/focus/selected/disabled/loading/empty/error。
- 每个域检查 dark + light。
- Overlay/Popover/Tooltip/Toast/Context Menu/Terminal/Preview 单独签收。
- V1 class 若仍靠 legacy.css 兜底，视为未迁移。