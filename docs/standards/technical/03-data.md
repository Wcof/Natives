# 技术架构 03 · 数据与持久化

> **版本**: 2.0.0 · **日期**: 2026-08-19
> **关联 ADR**: [ADR-0005](../../adr/0005-plugin-state-preservation-strategy.md)（状态分层）、[ADR-0008](../../adr/0008-electron-to-tauri-migration.md)（Tauri 迁移）
> **关联源文件**: `src-tauri/src/db.rs`、`src-tauri/src/env_manager.rs`

---

## 一、本篇要约束什么

数据层的约束决定系统能否安全演进。本篇钉死三件事：**数据存在哪**、**如何隔离**、**如何迁移**，外加**原子写入**与**状态分层**两条横切约束。

---

## 二、数据存放位置

#### R-D1 · 非 Secret 用户数据集中在 `~/.natives/`
- **等级**：MUST
- **分类**：数据、命名
- **规则**：除 OS Keychain 持有的 Secret 外，应用自有用户数据（SQLite、应用文件、日志）**必须**集中在 `~/.natives/` 目录下，结构如下：
  ```
  ~/.natives/
  ├── natives.db        # SQLite（WAL）
  ├── modules/          # 已安装模块
  ├── apps/             # Extension App 安装物（ADR-0025 D11）
  │   └── <appId>/
  │       ├── runtime/<version>/<host> + current
  │       ├── packages/        # kind=data 资源包
  │       ├── data/            # App 自有数据库（如 fund.db，只有该 App Host 可写）
  │       ├── cache/  imports/  staging/
  ├── migrations/       # 可恢复迁移 checkpoint（禁止明文 Secret）
  └── logs/             # 运行日志（apps/<appId>/ 子目录）
  ```
  Secret **必须**按 R-S12 进入 OS Keychain；DB 只保存 opaque reference。**禁止**把其它用户数据散落到项目目录或任意路径；**禁止**用 env 变量随意覆盖此根目录（除非有受控测试夹具）。

  **App 数据分账（ADR-0025）**：`natives.db` 内的 App Store 表（`apps` / `app_packages` / `app_permissions` / `app_install_transactions`）是 App 安装状态的唯一权威，只由 `native-file-host` 写入；App 业务数据库（如 `apps/fund/data/fund.db`）只由该 App 的 Runtime Host 写入。两者互不开放：Core Host 禁止打开 App DB，App Host 禁止访问 `natives.db`。卸载默认保留 `data/` 与 `imports/`；「删除应用及全部个人数据」必须二次确认。
- **为什么**：dotfile 目录模式与兄弟项目（CodePilot/Natives2）一致，便于备份、迁移、清理。
- **检查方法**：新增持久化路径时核对是否在 `~/.natives/` 下。

---

## 三、命名空间隔离

#### R-D2 · 插件数据按 module_id 命名空间隔离
- **等级**：MUST
- **分类**：数据、安全
- **规则**：插件的 KV 数据（`module_data` 表）**必须**以 `(module_id, key)` 复合主键隔离。任何 Bridge `db.get/set/list` 调用**必须**由 Main 强制注入调用方的 `module_id`，**禁止**接受插件自报的 module_id。
- **正例**：插件 A 调 `natives.db.get('x')` → Main 用 Token 反查出的 moduleId 拼 key。
- **反例**：让插件在参数里传 `moduleId: 'com.B'` 去读 B 的数据 → 违反。
- **为什么**：命名空间隔离是插件互不干扰的数据底线；信任插件自报 id 等于没有隔离。
- **检查方法**：Bridge `db.*` handler 是否忽略请求参数里的 moduleId、改用 Token 反查。

---

## 四、Schema 迁移

#### R-D3 · 用增量迁移，不改表重建
- **等级**：MUST
- **分类**：数据
- **规则**：DB schema 变更**必须**用增量迁移：启动时 `PRAGMA table_info()` 检查现有列，用 `ALTER TABLE ADD COLUMN` 补齐缺失列。**禁止** `DROP TABLE` 重建（会丢用户数据）。表结构变更需配合文件锁防并发迁移。
- **正例**：`src-tauri/src/db.rs` 的 `apply_migrations()` 函数已是增量模式。
- **为什么**：用户数据不可丢；重建表在已发布版本上是事故。
- **检查方法**：新增字段是否走 `ALTER`；是否有 `DROP TABLE`。

#### R-D3.1 · 主题持久化权威收敛与 v30 增量迁移
- **等级**：MUST
- **分类**：数据、主题
- **规则**：
  - 增量迁移 **v30** 必须将全局主题单一权威收敛到 `settings:theme`。
  - 迁移判定优先级：有效且存在的 `settings:theme` > 活跃 workspace legacy theme > `dark`。
  - 迁移过程必须幂等、事务化，**严禁**让历史 Workspace 主题覆盖用户已保存的全局偏好。
  - `workspaces.theme` 物理列保留用于迁移审计与回滚证据，生产读写与 DTO 全部切断，只有在 death proof 建立后才能另行清理。
- **为什么**：见 ADR-0022。消除 Workspace 与 Settings 的双写与竞态覆盖。

#### R-D4 · 新表/新字段必须开 WAL 与外键
- **等级**：MUST
- **分类**：数据
- **规则**：DB 初始化**必须**启用 `PRAGMA journal_mode=WAL` 与 `PRAGMA foreign_keys=ON`。外键**应该**带 `ON DELETE CASCADE` 或 `SET NULL` 明确级联策略。新增表需在本篇附录登记（见文末）。
- **为什么**：WAL 提升并发写性能；外键保证引用完整性。
- **检查方法**：`database.ts` 初始化含两个 PRAGMA。

---

## 五、原子写入

#### R-D5 · 配置与文件写入用「临时文件 + fsync + rename」
- **等级**：MUST
- **分类**：数据
- **规则**：覆盖重要文件（配置、凭证、用户文档）时**必须**用原子写入：写到临时文件 → `fsync` → `rename` 覆盖原文件。涉及并发编辑（如 Agent 与用户同时改同一文件）时**必须**用 mtime 冲突检测。
- **为什么**：崩溃或并发写入会导致半写文件损坏，对凭证/文档是灾难。
- **检查方法**：新增文件写入逻辑时核对是否原子写；`state-persistence.ts` 是否已遵循。

---

## 六、状态分层（插件状态保留）

承接 ADR-0005。

#### R-D6 · 插件状态遵循热/温/冷/持久四层
- **等级**：SHOULD
- **分类**：状态、性能
- **规则**：插件 iframe 状态**应该**按四层管理：

| 层 | 含义 | 何时进入 |
|----|------|---------|
| 热 | 当前可见，JS 内存保留 | 用户正在使用 |
| 温 | 最近 N 个后台 iframe，隐藏但保留 | 切换走，未达上限 |
| 冷/销毁 | 超出温层上限，销毁 iframe | LRU 淘汰 |
| 持久 | 插件主动 `natives.db.set()` 保存 | 插件显式调用 |

温层上限**应该**可配置（默认约 5）。销毁前**应该**给插件 `beforeunload` 机会存盘。
- **为什么**：见 ADR-0005。分层在内存与体验间取平衡。
- **检查方法**：`iframe-manager.ts` 的 LRU 与心跳逻辑是否覆盖四层。

---

## 七、本篇合规自检清单

- [ ] 非 Secret 数据在 `~/.natives/`；Secret 在 OS Keychain；DB 只有 opaque reference（R-D1/R-S12）。
- [ ] 插件数据按 module_id 隔离，且 moduleId 来自 Token 反查而非插件自报（R-D2）。
- [ ] schema 变更走增量 `ALTER`，没有 `DROP TABLE`（R-D3）。
- [ ] 新表启用了 WAL + 外键级联策略（R-D4）。
- [ ] 重要文件写入用了原子写 + mtime 冲突检测（R-D5）。

---

## 附录：当前表清单

> 新增表请在此登记，并补 `ALTER` 迁移逻辑。schema 版本见 `settings._schema_version`（v8 起增量演进，当前 head v28；PWSV2 追加 v29，见 ADR-0021 修订 §PWSV2）。

| 表 | 用途 |
|----|------|
| `modules` | 模块注册表 |
| `module_permissions` | 模块权限声明 |
| `settings` | 用户设置（KV） |
| `module_data` | 插件数据（按 module_id 隔离） |
| `workshop_cache` | 创意工坊元数据缓存 |
| `env_profiles` | 环境配置组 |
| `external_creative_apps` | 外部 GitHub 容器创意应用（ADR-0013，v7） |
| `creative_app_env` | 外部应用 env（AES-GCM，级联删除） |
| `local_creative_apps` | 本地项目创意的启动方案与运行状态（v8） |
| `local_creative_env` | 本地项目 env（AES-GCM，级联删除，v8） |
| `env_variables` | 环境变量（加密） |
| `notifications` | 通知历史 |
| `module_order` | 侧边栏排序 |
| `permission_audit_log` | 权限审计日志 |
| `usage_dashboard_snapshots` | 用量看板快照缓存（按 time_zone 主键，v6） |
| `workspaces` / `workspace_open_tabs`* / `workspace_context_items` / `workspace_widgets` / `workspace_layouts` / `workspace_view_states` / `workspace_tool_profiles` | Workspace V2 七表（v27 建表） |
| `workspace_templates` | Workspace 内置/个人模板 manifest（PWSV2，v29） |

\* 旧内容 tab 表 `workspace_tabs`（v27）PWSV2 起降为 legacy（无 production 读/写）；Workspace 会话由新表 `workspace_open_tabs`（v29）承载，death proof 后删除旧表。
