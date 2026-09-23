# TokenUsage 模块架构设计与契约规范

> 版本：1.0.0 · 日期：2026-09-20  
> 依据：ADR-0031（统一内置应用运行时）、技术标准 06（`docs/standards/technical/06-built-in-modules.md`）、技术标准 02（安全）、技术标准 03（数据）及迁移项目 `/Users/ldh/Downloads/project/clone/token-monitor`

---

## 1. 架构定位与执行原则（ADR-0031 / R-A1 ~ R-A10）

### 1.1 核心定位
`tokenusage` 是 Natives Monorepo 的官方内置模块（`modules/tokenusage`），用于跨工具、跨模型、跨设备聚合统计 AI 编程工具（Claude Code、Codex、Cursor、OpenCode、Pi 等 30+ 种工具）的 Token 消耗、费用估算、额度限制（Limits）与历史会话明细，并联动 macOS 顶部系统菜单栏（NSStatusItem）提供实时常驻状态排版。

### 1.2 关键约束
1. **单一二进制与单一 Native Host（R-A1 / R-A5）**：
   - 源码位于 `modules/tokenusage`，作为 Library crate 静态编译进单一产品级二进制 `natives-app-runtime`。
   - 操作系统 Native Messaging 只注册 `com.natives.app_runtime`（本地开发 `com.natives.local.app_runtime`），严禁注册独立的 `tokenusage-host`。
2. **独立按需进程与生命周期回收（R-A5 / R-A7）**：
   - 用户打开 `app.html?app=tokenusage` 时，由 Chrome 按需启动一个独立的 `natives-app-runtime` 进程实例。
   - 页面关闭或 Native Port 断开（stdin EOF）后，进程必须在 $\le 2$ 秒内安全彻底退出，释放 SQLite、网络和内存资源（堆内存 $\approx 0$ 驻留）。
3. **数据隔离（R-A8）**：
   - 业务数据严格存放于 `~/.natives/apps/tokenusage/data/tokenusage.db`，由当前 Runtime 进程独占读写。
   - 严禁在用户目录存放可执行文件。
4. **零假数据原则**：
   - 不得注入虚假/占位种子数据；未接入或未计价工具明确标注 `unpriced` / `unknown`，零消耗如实显示为 0。

---

## 2. 数据库设计（SQLite Schema & Migrations）

数据库路径：`~/.natives/apps/tokenusage/data/tokenusage.db`  
WAL 模式 + 外键开启 + busy_timeout = 2000ms。  
当前 schema 版本：`PRAGMA user_version = 1`。

### 2.1 数据表 DDL

```sql
-- 1. 采集来源与扫描游标表
CREATE TABLE IF NOT EXISTS usage_sources (
    id TEXT PRIMARY KEY,               -- 工具标识，如 'claude-code', 'codex', 'cursor', 'opencode'
    display_name TEXT NOT NULL,        -- 显示名称
    category TEXT NOT NULL,            -- 'cli', 'ide', 'api', 'proxy'
    enabled INTEGER NOT NULL DEFAULT 1,
    custom_scan_path TEXT,             -- 用户自定义扫描路径（可选）
    last_scanned_at TEXT,              -- 上次成功扫描时间 ISO-8601
    last_cursor TEXT,                  -- 扫描游标（文件 offset 或时间戳）
    status TEXT NOT NULL DEFAULT 'ok', -- 'ok', 'error', 'unsupported'
    error_message TEXT,
    updated_at TEXT NOT NULL
);

-- 2. 会话主表
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES usage_sources(id),
    title TEXT NOT NULL DEFAULT '',
    project_path TEXT NOT NULL DEFAULT '',
    started_at TEXT NOT NULL,
    last_used_at TEXT NOT NULL,
    total_input_tokens INTEGER NOT NULL DEFAULT 0,
    total_output_tokens INTEGER NOT NULL DEFAULT 0,
    total_cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    total_cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    total_reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_micros INTEGER NOT NULL DEFAULT 0, -- 费用以微美元（1 USD = 1,000,000 micros）定点存储
    message_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_source_time ON sessions(source_id, last_used_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(last_used_at DESC);

-- 3. 详细用量事件记录表
CREATE TABLE IF NOT EXISTS usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    record_hash TEXT UNIQUE,           -- 稳定记录指纹，用于幂等去重
    source_id TEXT NOT NULL REFERENCES usage_sources(id),
    session_id TEXT REFERENCES sessions(session_id),
    turn_id TEXT,
    model TEXT NOT NULL DEFAULT 'unknown',
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    cost_micros INTEGER NOT NULL DEFAULT 0,
    recorded_at TEXT NOT NULL,         -- 事件发生时间 ISO-8601
    raw_metadata TEXT                  -- JSON 格式扩展元数据
);
CREATE INDEX IF NOT EXISTS idx_records_source_model ON usage_records(source_id, model, recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_records_time ON usage_records(recorded_at DESC);

-- 4. 每日聚合统计表（秒级渲染热力图与趋势）
CREATE TABLE IF NOT EXISTS daily_aggregates (
    date TEXT NOT NULL,                -- YYYY-MM-DD
    source_id TEXT NOT NULL,
    model TEXT NOT NULL,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cost_micros INTEGER NOT NULL DEFAULT 0,
    session_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, source_id, model)
);
CREATE INDEX IF NOT EXISTS idx_daily_date ON daily_aggregates(date);

-- 5. 提供方额度（Limits）快照表
CREATE TABLE IF NOT EXISTS limits_cache (
    provider_id TEXT NOT NULL,         -- 如 'claude', 'codex', 'cursor', 'openrouter'
    account_id TEXT NOT NULL,          -- 账号 ID / email / key 别名
    window_kind TEXT NOT NULL,         -- 'session', 'daily', 'weekly', 'billing', 'credits'
    label TEXT NOT NULL DEFAULT '',
    used_percent REAL,
    remaining_percent REAL,
    used_units REAL,
    total_units REAL,
    unit_type TEXT NOT NULL DEFAULT 'percent', -- 'percent', 'tokens', 'usd', 'requests'
    resets_at TEXT,                    -- ISO-8601 重置时间
    fetched_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'ok',
    PRIMARY KEY (provider_id, account_id, window_kind)
);

-- 6. 模块配置表
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- 7. 多设备同步状态表
CREATE TABLE IF NOT EXISTS sync_state (
    device_id TEXT PRIMARY KEY,
    device_name TEXT NOT NULL,
    last_synced_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'idle',
    payload_hash TEXT
);
```

### 2.2 精度与费用计算规则
- 内部所有金额统一以 **微美元（1 USD = 1,000,000 micros，6 位定点整数）** 计算与持久化，杜绝 JavaScript 浮点数累加误差。
- Token 桶严格非重叠：
  $$\text{input\_total} = \text{input\_uncached} + \text{cache\_read} + \text{cache\_write}$$
  $$\text{output\_total} = \text{output\_non\_reasoning} + \text{reasoning}$$
  $$\text{total\_tokens} = \text{input\_total} + \text{output\_total}$$
- 货币换算支持在前端依据每日汇率快照格式化为 CNY、USD、EUR 等。

---

## 3. HTTP API 规范（App Runtime Protocol v2）

所有 API 均由 `natives-app-runtime` 在 `127.0.0.1:<port>` 上承载，前端调用需携带握手获得的 Bearer Token。

### 3.1 接口列表

| 方法 | 路由 | 说明 |
|---|---|---|
| `GET` | `/api/overview` | 获取统计总览（今日/本周/本月/总计 tokens & cost，活跃工具/模型） |
| `GET` | `/api/tools` | 获取各 AI 工具用量列表与细分（输入/输出/缓存命中率/费用） |
| `GET` | `/api/models` | 获取模型维度的聚合用量与成本列表 |
| `GET` | `/api/sessions` | 分页查询会话列表，支持按工具、时间范围、关键词筛选 |
| `GET` | `/api/sessions/detail` | 查询指定 Session 的详细 turn/message 拆解 |
| `GET` | `/api/limits` | 获取当前所有 AI 工具的额度窗口与百分比 |
| `POST` | `/api/limits/refresh` | 手动触发刷新指定或全部提供方额度 |
| `GET` | `/api/trends` | 获取指定日期区间内的每日用量趋势（支持按工具/模型堆叠） |
| `GET` | `/api/devices` | 获取多设备同步列表与各设备状态 |
| `GET` | `/api/settings` | 获取用户配置（货币、追踪工具、扫描路径、托盘排版） |
| `POST` | `/api/settings` | 更新用户配置 |
| `POST` | `/api/collect` | 手动触发全量或增量本地日志采集 |
| `GET` | `/api/tray/state` | 获取供 macOS 系统菜单栏（NSStatusItem）渲染的紧凑数据 |

### 3.2 重点接口 Payload 契约

#### `GET /api/overview`
```json
{
  "currency": "USD",
  "periods": {
    "today": {
      "totalTokens": 1250400,
      "costUsd": 3.42,
      "inputTokens": 980000,
      "outputTokens": 270400,
      "cacheHitRate": 0.68,
      "sessionCount": 14
    },
    "thisWeek": { "totalTokens": 8420000, "costUsd": 21.80, "sessionCount": 62 },
    "thisMonth": { "totalTokens": 32150000, "costUsd": 84.50, "sessionCount": 210 },
    "allTime": { "totalTokens": 142000000, "costUsd": 380.20, "sessionCount": 980 }
  },
  "topTools": [
    { "sourceId": "claude-code", "name": "Claude Code", "totalTokens": 720000, "costUsd": 2.10 },
    { "sourceId": "codex", "name": "Codex", "totalTokens": 410000, "costUsd": 1.12 }
  ],
  "topModels": [
    { "model": "claude-3-7-sonnet-20250219", "totalTokens": 680000, "costUsd": 2.04 },
    { "model": "gpt-4o", "totalTokens": 350000, "costUsd": 0.95 }
  ],
  "lastCollectedAt": "2026-09-20T08:30:00Z"
}
```

#### `GET /api/tray/state`（专供 macOS 菜单栏排版使用）
```json
{
  "mode": "both",               // 'tokens', 'cost', 'both', 'bars', 'custom'
  "displayText": "1.25M · $3.42",
  "tooltip": "Token Monitor: 1,250,400 tokens today ($3.42)",
  "worstLimit": {
    "providerId": "claude",
    "windowKind": "session",
    "remainingPercent": 28.0,
    "resetsAt": "2026-09-20T12:00:00Z"
  },
  "quickItems": [
    { "id": "claude", "name": "Claude Code", "tokens": "720K", "remaining": "28%" },
    { "id": "codex", "name": "Codex", "tokens": "410K", "remaining": "75%" }
  ]
}
```

---

## 4. 本地多工具采集引擎（30+ AI 工具覆盖）

采集引擎移植自 `token-monitor` 与 `tokscale` 数据契约，覆盖 30+ 种工具：

### 4.1 支持工具矩阵

| 工具分类 | 工具名称 | 数据源路径 / 机制 | 采集指标 |
|---|---|---|---|
| **核心 CLI** | Claude Code | `~/.claude/projects/`, `~/.claude/transcripts/` | Tokens, 费用, Session, Cache 命中 |
| | Codex | `~/.codex/sessions/`, `archived_sessions/` | Tokens, 费用, Session 明细 |
| | OpenCode | `~/.local/share/opencode/` (`opencode*.db`, `storage/`) | Tokens, 费用, SQLite 增量 |
| | Hermes Agent | `~/.hermes/state.db` | Tokens, 费用 |
| | OpenClaw | `~/.openclaw/agents/` | Tokens |
| | Kimi CLI / Code | `~/.kimi/sessions/`, `~/.kimi-code/sessions/` | Tokens, 额度 |
| | Qwen CLI | `~/.qwen/projects/` | Tokens |
| | Grok Build | `~/.grok/sessions/`, `logs/unified.jsonl` | Tokens, 额度 |
| | Pi / Oh My Pi | `~/.pi/agent/sessions/`, `~/.omp/agent/sessions/` | Tokens, Session |
| | ZCode / GLM | `~/.zcode/projects/`, `cli/db/db.sqlite` | Tokens, 额度 |
| **IDE / 扩展** | Cursor | `~/.config/tokscale/cursor-cache/` | 账号用量, 额度 |
| | GitHub Copilot | VS Code `workspaceStorage/*/chatSessions/`, `~/.copilot/` | Tokens, 额度 |
| | Antigravity | `~/.gemini/antigravity/` | Tokens, 额度 |
| | Cline | VS Code globalStorage tasks, `~/.cline/data/` | Tokens, Tasks |
| | Zed | `~/.local/share/zed/threads/threads.db` | Tokens, 额度 |
| | Cherry Studio | AppData `CherryStudio/Data/Agents/` | Tokens |
| | LM Studio | `~/.lmstudio/server-logs/**/*.log` | Tokens (OpenAI API 兼容) |
| **API / 云额度** | OpenRouter | Management API 密钥 | 额度, 余额, 用量 |
| | Minimax | Minimax API 密钥 | Token Plan 额度 |
| | Volcengine | 火山方舟 AK/SK / API 密钥 | Coding Plan 额度 |
| | Alibaba Cloud | 百炼 / Model Studio Cookie/API | Token Plan 额度 |
| | Third-party | New API / One API 兼容端点 | 余额与用量 |

### 4.2 增量读取与防重重叠策略
1. **文件指纹与已读 Offset 记录**：对日志与 transcript 采用 `(file_path, file_size, file_mtime, last_offset)`，杜绝全量重复扫描。
2. **唯一 Hash 键去重**：单条记录生成稳定指纹 `SHA256(sourceId + sessionId + turnId + recordedAt + tokens)`，入库使用 `INSERT OR IGNORE`。
3. **有界扫描与取消信号**：单次扫描时长 $\le 2$ 秒，每次批量最多处理 1000 条记录并检查 `CancellationToken`，防止卡顿。

---

## 5. macOS 菜单栏（标签栏）架构与集成设计

### 5.1 架构方案
macOS 顶部系统菜单栏（NSStatusItem）是用户的常驻入口。在 Natives 整体架构下：
1. **宿主承载**：
   - 生产环境：由 macOS 主入口包装器 `installers/macos/resources/launcher-main.m` 扩展，或由轻量级辅助常驻进程 `NativesMenuBar` 提供 `NSStatusItem`。
   - `launcher-main.m` 已经具备 AppKit、主事件循环与 `NSTask` 运行环境，扩展支持常驻菜单栏模式。
2. **通信通道**：
   - `NativesMenuBar` 定时（或响应唤起）通过本地 IPC / loopback 查询 `tokenusage` 模块的 `/api/tray/state`。
   - 数据为紧凑只读快照，查询开销 $< 5\text{ms}$。
3. **状态栏渲染特性**：
   - **Template Image 自适应**：macOS 菜单栏图标采用 `isTemplate = YES`，自动适应系统的深色/浅色模式及选中反色。
   - **排版支持**：
     - 单图标模式（仅显示 Token Monitor 标志图标）
     - 今日 Token 模式（如 `⚡ 1.25M`）
     - 今日费用模式（如 `⚡ $3.42`）
     - 组合模式（如 `1.25M · $3.42`）
     - 额度进度条模式（以微型图形绘制最低剩余额度百分比条）
4. **交互联动**：
   - **点击弹出快捷菜单（NSPopover / NSMenu）**：展示核心工具快速看板（今日消耗、Top 工具、Claude/Codex 额度剩余）。
   - **一键打开 Natives**：点击“打开 Token Usage 仪表板”或托盘标题，调用 `openInChrome("app.html?app=tokenusage")`，若 Chrome 已打开则直接激活该标签页。

---

## 6. 多设备 Hub 同步协议

兼容 `token-monitor` Hub 协议：
1. **模式选择**：
   - 本地独立模式（默认，无需服务器）。
   - 连接到 Hub 模式（通过 SSE 接收远程设备数据推送并上报本机聚合统计）。
   - 托管 Hub 模式（本地启动轻量 HTTP/SSE 服务，供局域网设备接入）。
2. **安全隔离**：
   - Hub 传输仅包含汇总统计（聚合 Token、费用、模型分类），绝对不传输用户 Prompt、代码、敏感路径或密钥。

---

## 7. 验收与合规自检（针对 R-A1 ~ R-A10）

- [x] **R-A1 模块交付**：`tokenusage` 作为 Monorepo 内置模块，不设立独立发布版本。
- [x] **R-A2 唯一身份**：使用稳定 appId `tokenusage`，不注册独立 `.app`。
- [x] **R-A3 初始化分离**：初次打开由当前用户创建 `tokenusage.db`，安装包不写用户目录。
- [x] **R-A4 清单一致**：在 `modules/registry.json` 中声明，受构建期校验保护。
- [x] **R-A5 统一 Runtime**：编译入 `natives-app-runtime`，使用 `com.natives.app_runtime`。
- [x] **R-A6 通用 Owner Page**：通过 `app.html?app=tokenusage` 握手并承载 sandbox iframe。
- [x] **R-A7 退出时效**：stdin EOF 后 $\le 2$ 秒彻底退出并回收全部资源。
- [x] **R-A8 数据隔离**：所有业务数据写入 `~/.natives/apps/tokenusage/data/`。
- [x] **R-A9 迁移支持**：具备 `PRAGMA user_version` 校验与 `.migration.json` journal 恢复。
- [x] **R-A10 本地验收**：支持本地开发模式隔离测试。
