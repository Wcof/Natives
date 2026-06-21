# 技术架构 03 · 数据与持久化

> **版本**: 1.1.0 · **日期**: 2026-06-19
> **关联 ADR**: [ADR-0005](../../adr/0005-plugin-state-preservation-strategy.md)（状态分层）、[ADR-0008](../../adr/0008-electron-to-tauri-migration.md)（Tauri 迁移）
> **关联源文件**: `src-tauri/src/db.rs`、`src-tauri/src/env_manager.rs`

---

## 一、本篇要约束什么

数据层的约束决定系统能否安全演进。本篇钉死三件事：**数据存在哪**、**如何隔离**、**如何迁移**，外加**原子写入**与**状态分层**两条横切约束。

---

## 二、数据存放位置

#### R-D1 · 用户数据只存在 `~/.natives/`
- **等级**：MUST
- **分类**：数据、命名
- **规则**：所有用户数据（SQLite、模块文件、凭证、日志）**必须**存放在 `~/.natives/` 目录下，结构如下：
  ```
  ~/.natives/
  ├── natives.db        # SQLite（WAL）
  ├── modules/          # 已安装模块
  ├── env/              # 加密凭证
  └── logs/             # 运行日志
  ```
  **禁止**把用户数据散落到系统其它位置（如 `~/Library/`、项目目录）。**禁止**用 env 变量随意覆盖此根目录（除非有受控的测试夹具）。
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

- [ ] 我的持久化路径在 `~/.natives/` 下（R-D1）。
- [ ] 插件数据按 module_id 隔离，且 moduleId 来自 Token 反查而非插件自报（R-D2）。
- [ ] schema 变更走增量 `ALTER`，没有 `DROP TABLE`（R-D3）。
- [ ] 新表启用了 WAL + 外键级联策略（R-D4）。
- [ ] 重要文件写入用了原子写 + mtime 冲突检测（R-D5）。

---

## 附录：当前表清单（10 张）

> 新增表请在此登记，并补 `ALTER` 迁移逻辑。

| 表 | 用途 |
|----|------|
| `modules` | 模块注册表 |
| `module_permissions` | 模块权限声明 |
| `settings` | 用户设置（KV） |
| `module_data` | 插件数据（按 module_id 隔离） |
| `workshop_cache` | 创意工坊元数据缓存 |
| `env_profiles` | 环境配置组 |
| `env_variables` | 环境变量（加密） |
| `notifications` | 通知历史 |
| `module_order` | 侧边栏排序 |
| `permission_audit_log` | 权限审计日志 |
