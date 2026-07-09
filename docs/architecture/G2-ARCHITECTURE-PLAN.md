# G2 产品信息架构与数据模型重规划

> 日期: 2026-07-08 · 基于 G0 审计 + G1 参考分析

---

## 1. 新信息架构 (IA)

```
AiNative
├── 🏠 首页 (/)
│   ├── 系统状态卡片
│   ├── 快速操作入口
│   └── 最近活动
│
├── 📂 资产管理 (/library)
│   ├── 文件夹树 (左)
│   ├── Tag 筛选器
│   ├── 项目列表 + 搜索
│   ├── 项目详情/编辑/删除
│   ├── 批量操作工具栏
│   └── 统计面板
│
├── 💬 AI 工作台 (/ai)
│   ├── 会话列表
│   ├── 消息输入/输出
│   ├── 执行日志
│   ├── Subagent 管理 (/subagents)
│   └── 执行引擎面板
│
├── 📁 文件浏览器 (/files)
│   ├── 目录导航
│   ├── 文件列表/网格
│   └── 预览面板
│
├── 🧩 模块管理 (/modules)
│   ├── 已安装列表
│   ├── 启用/停用
│   └── 扫描
│
├── 🛍️ 商店 (/store)
│   ├── 浏览/搜索
│   └── 安装
│
├── 🔧 工具 (/tools)
│
└── ⚙️ 设置
    ├── Provider/Key 管理
    ├── 主题/语言
    ├── 终端配置
    ├── 执行引擎设置
    └── 关于
```

### 用户主要路径
1. **配置**: 设置 → 添加 Provider → 输入 URL+Key → Test → 保存
2. **资产管理**: /library → 创建文件夹 → 添加项目 → 打 Tag → 搜索
3. **AI 执行**: 配置 Subagent → 绑定 Provider/Key → 运行 → 查看日志
4. **终端**: 打开终端 → 输入命令 → 查看输出 → 关闭 → 查看日志

---

## 2. 路由设计

| 路由 | 页面 | 用户目标 | 核心动作 | 依赖 Goal |
|------|------|---------|---------|-----------|
| `/` | Dashboard | 查看系统概览 | — | G3 |
| `/library` | 资产管理 | 管理项目/文件夹/Tag | CRUD + 搜索 + 统计 | G4 |
| `/ai` | AI 工作台 | 执行 AI 任务 | 发送消息、查看日志 | G6 |
| `/subagents` | Subagent | 配置/运行 Agent | CRUD + 运行 + 日志 | G8 |
| `/files` | 文件浏览器 | 浏览本地文件 | 导航 + 预览 | G3 |
| `/modules` | 模块管理 | 管理插件 | 启用/停用 | G3 |
| `/store` | 商店 | 安装模块 | 搜索 + 安装 | G3 |
| `/tools` | 工具 | 工具集合 | — | G3 |
| Settings (嵌入) | Provider/Theme | 配置 | CRUD + 测试 | G5/G6 |

### 页面状态 (基于 G0 审计)
- ✅ 真实可用: `/`, `/library`, `/subagents`
- ⚠️ 部分可用: `/ai`, `/files`, `/modules`, `/store`, `/tools`
- ❌ 不可运行: 无

---

## 3. 核心数据模型

### 现有表 (10 张，来自 standards/technical/03-data.md)

| 表 | 用途 | 状态 |
|----|------|------|
| `modules` | 模块注册表 | ✅ |
| `module_permissions` | 模块权限声明 | ✅ |
| `settings` | 用户设置 (KV) | ✅ |
| `module_data` | 插件数据 | ✅ |
| `workshop_cache` | 创意工坊缓存 | ✅ |
| `env_profiles` | 环境配置组 | ✅ |
| `env_variables` | 环境变量(加密) | ✅ |
| `notifications` | 通知历史 | ✅ |
| `module_order` | 侧边栏排序 | ✅ |
| `permission_audit_log` | 权限审计日志 | ✅ |

### G4 新增表 (3 张)

| 表 | 用途 | 字段 | 外键 | 级联 |
|----|------|------|------|------|
| `folders` | 文件夹 | id, parent_id, name, sort_order, created_at, updated_at | parent_id → folders(id) | CASCADE |
| `tags` | 标签 | id, name, color, created_at | — | — |
| `items` | 项目 | id, folder_id, title, description, url, status, sort_order, created_at, updated_at | folder_id → folders(id) | SET NULL |
| `item_tags` | 项目-标签关联 | item_id, tag_id | item_id → items(id), tag_id → tags(id) | CASCADE, CASCADE |

### G8 新增表 (2 张)

| 表 | 用途 | 字段 | 外键 | 级联 |
|----|------|------|------|------|
| `subagents` | Subagent 配置 | id, name, role, instructions, tools, provider_id, key_id, created_at, updated_at | — | — |
| `subagent_runs` | Subagent 运行记录 | id, subagent_id, status, input, output, error, started_at, finished_at | subagent_id → subagents(id) | CASCADE |

### G5 新增表 (4 张)

| 表 | 用途 | 字段 | 外键 | 级联 |
|----|------|------|------|------|
| `user_providers` | Provider 配置 | id, preset_name, name, website_url, base_url, created_at, updated_at | — | — |
| `provider_api_keys` | API Key | id, provider_id, label, api_key_encrypted, dek_encrypted, created_at | provider_id → user_providers(id) | CASCADE |

### 总体表结构

```
12 张业务表 + 10 张基础表 = 22 张表
所有新表使用: WAL + FOREIGN KEYS + 增量迁移 + created_at/updated_at
```

---

## 4. Key 安全数据流

```
用户输入明文 Key (前端 Input)
    │ type="password"
    ▼
window.nativesAPI.provider.addKey({ apiKey: "...", ... })
    │ IPC invoke
    ▼
[Tauri Backend - Main Process]
    │ 1. 信封加密 (KEK-DEK, AES-256-GCM)
    │ 2. 持久化到 provider_api_keys.api_key_encrypted
    │ 3. 构建 DTO: masked_key 替代 api_key
    ▼
返回 { maskedKey: "sk-a…1b2c", ... } 给前端展示

[Runtime 执行时]
    │ 1. 从 DB 读取 api_key_encrypted + dek_encrypted
    │ 2. 信封解密 → 明文 Key
    │ 3. 注入子进程 env (不返回前端)
    │ 4. 日志走 log_sanitizer 脱敏
    ▼
子进程使用完整 Key 调用 API
```

---

## 5. 状态管理

| 状态 | 归属 | 管理方式 |
|------|------|----------|
| 主题/语言 | ShellLayout 全局 | localStorage + DB settings |
| Provider 列表 | SettingsPage 局部 | 加载时从后端拉取 |
| 资产管理数据 | LibraryPage 局部 | 加载时从后端拉取 |
| Subagent 数据 | SubagentPage 局部 | 加载时从后端拉取 |
| 终端会话列表 | ShellLayout 全局 | useTerminalSessions Hook |
| 通知 | ShellLayout 全局 | Tauri 事件广播 |

**规则:** 跨页面全局状态集中在 ShellLayout，业务数据按页面局部加载。

---

## 6. 终端与 Agent 架构

```
┌─────────────────────────────────────────────┐
│                 Renderer                     │
│  Terminal.tsx ← useTerminalSessions.ts      │
│  AssistantWorkbench ← RuntimePanel           │
│  SubagentPage / LibraryPage                  │
└────────────────┬────────────────────────────┘
                 │ window.nativesAPI (IPC)
                 ▼
┌─────────────────────────────────────────────┐
│            Tauri Main Process                │
│  ┌──────────┐  ┌───────────┐                │
│  │ terminal │  │  runtime  │                │
│  │ commands │  │  registry │                │
│  └────┬─────┘  └─────┬─────┘                │
│       │              │                      │
│  ┌────▼─────┐  ┌─────▼──────┐               │
│  │ PTY      │  │ Native     │               │
│  │ Manager  │  │ Engine     │               │
│  └──────────┘  └─────┬──────┘               │
│                      │                      │
│              ┌───────▼──────┐                │
│              │ CLI Engine   │                │
│              │ (Claude/Codex)│               │
│              └───────┬──────┘                │
│                      │ spawn + env inject    │
│              ┌───────▼──────┐                │
│              │  Subprocess  │                │
│              │  Watchdog    │                │
│              └──────────────┘                │
└─────────────────────────────────────────────┘
```

**进程生命周期:**
1. 创建 → 启动 → 运行 → 完成/失败/取消
2. 取消时: 发送 SIGTERM → 等待 5s → SIGKILL
3. Drop/Exit 时: 清理所有子进程
4. 端口: 动态分配，释放后归还

---

## 7. 合规检查

- ✅ 对照 G0 空壳清单: 6 处低优先级 placeholder 各有归宿
- ✅ 对照 G1 复刻矩阵: 25+ 项能力已有落地 Goal
- ✅ 对照 standards: 所有 DB 变更走增量迁移、Key 加密、日志脱敏
