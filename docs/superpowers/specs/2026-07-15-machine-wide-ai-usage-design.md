# 全机 AI 用量总账设计

## 目标

个人主页统计当前电脑上所有可发现 AI 应用的本地用量，不局限于当前项目。只展示应用真实保存的 Token、费用、会话和时长；本地没有可靠用量字段时显示“已检测，但无法可靠统计”，不按消息字数估算。

“全机”指应用有权限读取的本机用户目录、标准系统应用数据目录、自定义工具 Home 和已挂载数据卷。同步过程不递归遍历整个文件系统，而是通过已知来源目录、环境变量和应用安装信息发现数据，避免每次同步扫描无关文件。

## 来源范围

首版覆盖本机已经存在或具有稳定公开日志格式的来源：

| 来源 | 数据入口 | 首版能力 |
| --- | --- | --- |
| Claude CLI | `CLAUDE_CONFIG_DIR`、`~/.claude/projects` | Token、缓存、模型、项目、会话、时间 |
| Codex | `CODEX_HOME`、`~/.codex/sessions`、`~/.codex/archived_sessions` | Token、缓存、模型、项目、会话、时间 |
| Atomcode | `ATOMCODE_HOME`、`~/.atomcode/sessions` | 当前和归档会话、有效 Token、缓存、项目、会话、时长 |
| OpenCode | XDG 数据目录中的 `opencode.db` | Token、缓存、费用、模型、项目、会话、时间 |
| Gemini CLI | `GEMINI_CLI_HOME` 或 `~/.gemini/tmp/*/chats` | 日志存在时统计 Token、缓存、模型、会话、时间 |
| Grok CLI | `GROK_HOME` 或 `~/.grok/sessions` | `signals.json` 存在时统计 Token、模型、会话、时间 |
| Natives | 当前应用数据库 | 已有真实消息与 Token 数据 |

Cursor、Claude Desktop、ChatGPT Desktop 和 Antigravity 首版只做安装或数据目录检测。本地没有稳定、可验证 Token 明细时，来源状态显示为 `detected`/“无法可靠统计”，不进入总 Token 与费用聚合。

新增工具通过一个静态来源清单加入：来源 ID、显示名、标准目录、自定义 Home 环境变量和扫描函数。首版不做插件系统、全盘文件索引或可编辑规则引擎。

## 统一统计口径

- 输入 Token 保留来源报告口径；Codex 的原始输入包含缓存输入。
- 缓存读取和缓存写入分别单列。
- 总 Token = 输入 + 输出 + 缓存读取 + 缓存写入；Codex 按原始输入再加缓存，以匹配 Codex 自身统计。
- 推理 Token 若来源明确说明包含在输出中则不重复相加；单独上报时并入输出。
- 费用只使用来源原始费用或项目已有可靠定价；未知为 `null`。
- 同一来源使用稳定的 `session_id + request/message/turn_id` 去重；代理日志和会话日志能证明为同一请求时只保留一份。
- 单个来源缺字段不影响其他来源；聚合时 `null` 不转换成伪造的零。

## Atomcode 修正

Atomcode 当前会话由 `.jsonl + .snapshot + .meta` 组成，归档会话是独立 `.json`。现实现只读取 `.jsonl`，漏掉归档会话，并把 Snapshot 中包含缓存的完整 Prompt 累加为普通输入。

修正后：

- 当前会话以 Snapshot 的逐请求 Token 为优先数据，使用 `prompt - cached` 作为新鲜输入，`completion` 作为输出，`cached` 单列；Snapshot 缺失时按 JSONL 同样归一化回退。
- 独立 `.json` 归档会话使用 `turn_stats.total_tokens` 作为可靠总量；缺少输入/输出拆分时保持拆分字段为 `null`。
- 归档只有会话级时间时使用 `updated_at` 归属日期，首末时间使用 `created_at/updated_at`，并标记为会话级精度。
- 当前与归档记录按 Atomcode session ID 去重。

## 数据流与缓存

用户点击“同步数据”时：

1. 来源清单解析标准目录和当前进程可见的自定义 Home。
2. 各来源并行读取本地日志或数据库，输出现有统一的日、小时和会话记录。
3. 聚合器按来源和稳定事件键去重，合并可信记录；仅检测到的应用生成来源状态，不生成用量。
4. 写入现有 dashboard snapshot，个人主页筛选继续只读快照。

提高快照 schema 版本，使旧的 Atomcode 高估数据不会继续显示。同步告警列出无法读取、格式不支持和仅检测无用量三种状态，但不阻塞其他来源。

## 错误处理

- 目录不存在表示未检测，不报错。
- 应用存在但没有会话表示已检测、零会话，不伪造 Token。
- 权限不足、数据库锁定或部分文件解析失败标记来源为 `partial`，保留成功记录。
- 未知日志版本不猜字段，显示格式不支持并保留旧快照中的其他来源。
- 其他 macOS 用户目录和受系统保护目录只有在应用实际获得权限时读取；不尝试提权。

## 验证

- Atomcode 回归：当前会话缓存归一化、归档 `.json` 纳入、当前/归档 session ID 去重、Snapshot 缺失回退。
- OpenCode、Gemini CLI、Grok CLI 使用最小真实格式 fixture 验证 Token、时间和去重。
- 来源发现测试覆盖默认目录、自定义 Home、已检测但无可靠用量。
- 聚合测试确保缓存进入总 Token、未知字段保持 `null`、同一事件不重复累计。
- 运行用量模块 Rust 测试、`cargo check`、前端类型检查与 `git diff --check`。

## 非目标

- 不估算没有 Token 字段的桌面应用用量。
- 不读取聊天正文来推算 Token。
- 不在每次同步时递归扫描整个系统盘。
- 不接入云端账户、账单或配额 API。
- 不为未知 AI 应用猜测私有存储格式。
