# Subagent A｜Workspace Host / Persistence / Rust Death

负责 Local-First Workspace 数据权威、SQLite v27、Repository/Service/IPC、旧 Home 迁移，以及后续 Rust Agent legacy 清退。

## 硬边界

- 不复制任何参考项目源码。
- 不越过 file ownership。
- 开发期间不要求跑测试；记录 deferred verification。
- 不为了方便建立第二套数据权威。
- 完成每个 Wave 后写 handoff。

## Task 顺序

### A-001｜创建 Rust workspace domain 模块骨架
- 依赖：M-004
- 主要范围：src-tauri/src/workspace/**
- 完成条件：model/repository/service/snapshot/errors 分层

### A-002｜定义 Workspace / WorkspaceTab / ContextItem / WidgetRecord / LayoutRecord / ViewState / ToolProfile Rust model
- 依赖：A-001
- 主要范围：src-tauri/src/workspace/model.rs
- 完成条件：serde/ts-rs 命名一致

### A-003｜设计并实现 DB migration v27：workspaces
- 依赖：A-002
- 主要范围：src-tauri/src/db/*
- 完成条件：支持 soft delete 与索引

### A-004｜v27：workspace_tabs 表
- 依赖：A-003
- 主要范围：src-tauri/src/db/*
- 完成条件：Close!=Delete 可表达

### A-005｜v27：workspace_context_items 表与索引
- 依赖：A-003
- 主要范围：src-tauri/src/db/*
- 完成条件：kind/payload/sort 可持久化

### A-006｜v27：workspace_widgets 表与索引
- 依赖：A-003
- 主要范围：src-tauri/src/db/*
- 完成条件：configVersion/appearance/z-index 可持久化

### A-007｜v27：workspace_layouts 表与复合主键
- 依赖：A-003
- 主要范围：src-tauri/src/db/*
- 完成条件：compact breakpoint 与 free 均可存

### A-008｜v27：workspace_view_states 表
- 依赖：A-003
- 主要范围：src-tauri/src/db/*
- 完成条件：UI state 与业务数据隔离

### A-009｜v27：workspace_tool_profiles 表
- 依赖：A-003
- 主要范围：src-tauri/src/db/*
- 完成条件：仅保存非敏感 profile/reference

### A-010｜更新 schema version 26→27 与 migration sequencing
- 依赖：A-003,A-009
- 主要范围：src-tauri/src/db.rs,src-tauri/src/db/db_migrations.rs
- 完成条件：旧库可单向升级

### A-011｜实现 WorkspaceRepository CRUD
- 依赖：A-003
- 主要范围：src-tauri/src/workspace/repository.rs
- 完成条件：create/get/list/update/delete

### A-012｜实现 Workspace tab session repository
- 依赖：A-004,A-011
- 主要范围：src-tauri/src/workspace/repository.rs
- 完成条件：open/close/pin/reorder

### A-013｜实现 active workspace state 与恢复逻辑
- 依赖：A-012
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：关闭 active 时选择确定性 fallback

### A-014｜实现 context item CRUD
- 依赖：A-005,A-011
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：批量/排序不跨域读数据

### A-015｜实现 widget record CRUD 与 batch config update
- 依赖：A-006,A-011
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：事务化批量更新

### A-016｜实现 compact layout read/save
- 依赖：A-007,A-011
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：一次 stop 对应一次事务 commit

### A-017｜实现 free layout read/save
- 依赖：A-007,A-011
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：支持 world rect/parent/z index JSON

### A-018｜实现 view state CRUD
- 依赖：A-008,A-011
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：viewKey 独立

### A-019｜实现 tool profile CRUD
- 依赖：A-009,A-011
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：无 secret 明文

### A-020｜实现 WorkspaceSnapshot 聚合查询
- 依赖：A-014,A-015,A-016,A-017,A-018,A-019
- 主要范围：src-tauri/src/workspace/snapshot.rs
- 完成条件：单 workspace 一次读取组成 snapshot

### A-021｜实现 session snapshot：opened tabs + active id + metadata
- 依赖：A-012,A-013
- 主要范围：src-tauri/src/workspace/snapshot.rs
- 完成条件：inactive 不带完整 widgets

### A-022｜实现 duplicate workspace 事务
- 依赖：A-020
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：复制 context/widgets/layout/view/tool profile 但新 id

### A-023｜实现 template source metadata/创建模板入口
- 依赖：A-022
- 主要范围：src-tauri/src/workspace/service.rs
- 完成条件：模板不复制 runtime session state

### A-024｜实现旧 settings:home_workspace 解析器
- 依赖：A-010
- 主要范围：src-tauri/src/workspace/migration.rs
- 完成条件：只接受 schemaVersion 1 有效数据

### A-025｜实现 legacy Home -> Default Workspace 幂等迁移
- 依赖：A-024,A-015,A-016
- 主要范围：src-tauri/src/workspace/migration.rs
- 完成条件：不会重复创建；旧 key 暂保留

### A-026｜写 migration marker/hash/backup metadata
- 依赖：A-025
- 主要范围：src-tauri/src/workspace/migration.rs
- 完成条件：可判断已迁移与来源 hash

### A-027｜实现 typed workspace Tauri commands
- 依赖：A-020,A-021,A-022,A-023
- 主要范围：src-tauri/src/commands/workspace.rs
- 完成条件：command 只调用 service

### A-028｜导出 TS contract / 更新绑定生成入口
- 依赖：A-027
- 主要范围：src/lib/workspace/contracts.ts
- 完成条件：前后端字段一致

### A-029｜实现前端 workspace client facade
- 依赖：A-028
- 主要范围：src/lib/workspace/client.ts
- 完成条件：UI 不直接 db.get/set

### A-030｜实现 workspace change event contract
- 依赖：A-029
- 主要范围：src/lib/workspace/events.ts
- 完成条件：只发送 workspace 域事件

### A-031｜输出 Wave1 handoff 与 shared handler patch intent
- 依赖：A-030
- 主要范围：按任务语义
- 完成条件：主 Agent可无猜测接线

### A-032｜Wave2：补充 workspace context reorder/batch APIs
- 依赖：I-008
- 主要范围：src-tauri/src/workspace/**
- 完成条件：大批量更新事务化

### A-033｜Wave2：补充 snapshot revision/version 字段支持 stale reconcile
- 依赖：A-032
- 主要范围：src-tauri/src/workspace/**
- 完成条件：前端可判定过期 snapshot

### A-034｜Wave2：定义未来 MCP exposure allowlist DTO（不实现 Agent runtime）
- 依赖：A-033
- 主要范围：src-tauri/src/workspace/**
- 完成条件：只复用 domain service，不直连 DB

### A-035｜Wave2 handoff
- 依赖：A-034
- 主要范围：按任务语义
- 完成条件：记录未测试风险

### A-D01｜Legacy Death：生成 Rust legacy production reference graph
- 依赖：I-012
- 主要范围：src-tauri,crates,src-agent-daemon
- 完成条件：区分可删与 Proxy/Provider 仍依赖资产

### A-D02｜将仍需的 provider/proxy codec 从旧 Agent ownership 解耦
- 依赖：A-D01
- 主要范围：crates/provider-adapters,src-tauri/src/proxy
- 完成条件：目标 Host 不依赖 Agent runtime 语义

### A-D03｜删除/下线 src-tauri agent/assistant_service/jobs/runtime 生产入口
- 依赖：A-D02
- 主要范围：src-tauri/src/**
- 完成条件：无 handler/production call 进入旧 runtime

### A-D04｜删除/下线 daemon 生产依赖与 sidecar 启动链
- 依赖：A-D02
- 主要范围：src-tauri/src/daemon,src-agent-daemon
- 完成条件：无 Agent daemon production authority

### A-D05｜提出 root Cargo/src-tauri Cargo 清理 patch 给 Main
- 依赖：A-D03,A-D04
- 主要范围：Cargo.toml,src-tauri/Cargo.toml
- 完成条件：共享文件只交 patch intent

### A-D06｜Rust legacy death handoff
- 依赖：A-D05
- 主要范围：按任务语义
- 完成条件：列出残余仅测试/历史 docs 引用

## Handoff 模板

```text
Completed Task IDs:
Files created:
Files modified:
Contract assumptions:
Migration/compat impact:
Known risks:
Deferred verification:
Shared-file patch intents:
Reference-source-copy statement: NO SOURCE COPIED
```


# V2 追加｜Theme Preference / Appearance Host

### V-001｜冻结 ThemePreference/AppearancePreference Host contract
- 依赖：A-028,M-004
- 范围：`src-tauri/src/**; src/lib/workspace/contracts.ts`
- 完成：全局主题与 workspace appearance 分责清晰；不复制 token 到 DB

### V-002｜实现主题偏好持久化兼容迁移
- 依赖：V-001
- 范围：`src-tauri/src/** settings/preferences`
- 完成：旧 terminal-volt/frosted-jasmine 可归一到 dark/light；无数据丢失

### V-003｜实现 workspace appearance_json 白名单校验
- 依赖：V-001,A-015
- 范围：`src-tauri/src/workspace/**`
- 完成：只允许 surfaceVariant/header/opacity 等实例级外观，不允许颜色 token 入库

### V-004｜清理旧主题别名的后端写入路径
- 依赖：V-002
- 范围：`src-tauri/src/**`
- 完成：运行时不再产生 terminal-volt/frosted-jasmine 新值，读取仍兼容

