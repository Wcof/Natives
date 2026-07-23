# 执行引擎与助理模块整改状态

## 当前状态

**最后更新**: 2026-07-23
**当前阶段**: 阶段 2 完成，阶段 3 待开始

---

## 已完成工作

### 阶段 0：立即止血和恢复可信验证 ✅

1. **Rust Warnings 修复** - 5 个 warnings 已修复
2. **TypeScript AssistantMethod 补齐** - 添加 `run.rewindPreview`、`task.wait`
3. **测试配置改进** - 添加 `test:all` 脚本
4. **暂时隐藏不安全能力** - 移除 `run.rewind`、`run.rewindPreview`
5. **协议同步验证** - `npm run protocol:check` 通过

### 阶段 1：统一 Daemon 数据权威 ✅

1. **Schema 调整**
   - Daemon `run` 表添加 `effort` 和 `runtime_id` 列
   - Host `assistant_runs` 添加 `effort` 列
   - Host `assistant_conversations.mode` CHECK 已支持 `'goal'`

2. **Task 表写入路径实现**
   - 创建 `task_store.rs` 实现任务持久化
   - 集成到 `production.rs` 和 `rpc.rs`
   - 测试通过

3. **数据库迁移修复**
   - 修复 Host-only 数据库缺少 `conversation` 表的问题
   - 自动创建 `conversation` 表并从 `assistant_conversations` 复制数据

### 阶段 2：重建可信本地工具运行时 ✅

1. **统一 ToolCallContext 结构**
   - 新增 `ToolCallContext` 结构
   - 实现 `resolve_path()` 方法

2. **更新 ToolHandler trait**
   - 修改 trait 签名，增加 `context` 参数
   - 更新所有工具实现

3. **更新 CapabilityGateway**
   - 修改 `execute` 方法接受 context 参数

4. **集成到 production.rs**
   - 在 `PermissionGatedTools` 中创建 `ToolCallContext`

---

## 最新修复

### 1. Hydration 不匹配修复

**问题**: `data-native-traffic` 属性在服务器端为 `"false"`，客户端为 `"true"`

**原因**: `detectNativeTrafficLights()` 在 render 路径同步调用——SSR 无 `window` 返回 `false`，macOS 客户端首帧返回 `true`，子树从 `window-controls` 变成 `native-traffic-spacer`。

**解决方案**:
```tsx
// SSR + 客户端首帧都为 false；挂载后再切换
const [usesNativeTrafficLights, setUsesNativeTrafficLights] = useState(false);
useEffect(() => {
  setUsesNativeTrafficLights(detectNativeTrafficLights());
}, []);
```

**文件**: `src/components/shell/Sidebar.tsx`

### 2. 数据库迁移修复（schema 版本表冲突）

**问题**: `conversation table missing after migrations at ~/.natives/assistant.db`

**根因**: Host（`src-tauri/src/daemon/data.rs`）与 Daemon（`src-agent-daemon`）共用同一 `assistant.db`，并共用 `_schema_version`。Host 的 `assistant_*` 迁移已写到 v14，Daemon 读到 `MAX(version)=14` 后**跳过**自己的 1–9 迁移，导致从未创建 unprefixed 的 `conversation` / `message` / `run` 等权威表。

**解决方案**:
1. Daemon 改用独立表 `_daemon_schema_version` 跟踪自己的迁移进度
2. 首启时若已有完整 canonical core（conversation+message+run+run_event）则 bootstrap 版本，避免重复；否则从 0 跑完全部 Daemon 迁移
3. Host 的 `_schema_version` 保持不动
4. 回归测试：`test_host_schema_version_does_not_skip_daemon_migrations`

**文件**:
- `src-agent-daemon/src/storage/mod.rs`
- `src-agent-daemon/src/storage/migrations.rs`
- `src-agent-daemon/tests/failure_injection.rs`

### 3. 事件监听器错误

**问题**: `undefined is not an object (evaluating 'listeners[eventId].handlerId')`

**状态**: 堆栈为 `user-script:`（浏览器扩展注入脚本），不是应用源码。与 Tauri event unregister 的竞态也可能在扩展干扰下暴露。

**建议**:
- 忽略扩展导致的噪声；用无扩展的 WebView / `tauri dev` 复核
- 应用侧继续保证组件卸载时清理 `listen` / `onDbStateChanged` 订阅
---

## 测试状态

| 测试套件 | 状态 | 说明 |
|----------|------|------|
| capability-gateway | ✅ 通过 | 41 passed |
| agent-core | ✅ 通过 | 28 passed |
| assistant-protocol | ✅ 通过 | 46 passed |
| natives-agent-daemon | ✅ 通过 | 157 passed, 6 ignored |
| storage | ✅ 通过 | 8 passed |

---

## 待完成任务

### 阶段 3：闭合 Session Harness、队列、权限与任务

1. **合并 Harness** - 统一 agent-core 和 daemon 的 SessionHarness
2. **明确交互语义** - 实现插话、取消后发送
3. **并行工具** - 实现只读工具并行执行
4. **权限与交互** - 完善权限系统
5. **统一任务** - 统一 task 表和子代理任务

### 阶段 4：Checkpoint/Rewind、上下文与助理 UI

1. **Checkpoint v2** - 实现完整的 checkpoint 和 rewind 功能
2. **上下文** - 实现上下文统计和自动压缩
3. **助理会话 UI** - 实现工具卡、任务、回滚 UI

---

## 变更记录

### 2026-07-23
- 修复 Sidebar hydration 不匹配问题
- 修复数据库迁移 conversation 表缺失问题
- 更新整改状态文档

### 2026-07-23 (阶段 2)
- 完成 ToolCallContext 统一
- 更新 ToolHandler trait
- 集成到 production.rs

### 2026-07-23 (阶段 1)
- Daemon `run` 表添加 `effort` 和 `runtime_id` 列
- 创建 `task_store.rs` 实现任务持久化

### 2026-07-23 (阶段 0)
- 完成 Rust warnings 修复
- 完成 TypeScript AssistantMethod 补齐
- 暂时隐藏 run.rewind 和 run.rewindPreview
