# Blueprint: Term_RecordingMeta (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
录像cast文件私有元信息(fanbox字段)，记录cwd/cols/rows/startedAt/theme，回放/列表用这些元信息，asciinema标准解析器会忽略未知字段，空终端(小于700字节)不进列表防噪音。

### Reference Code Analysis
- **Target File:** `src-tauri/src/terminal/recordingmeta.rs`
- **Referenced Functions:** —
- **Optimized Implementation Logic（Natives Rust 优化版）:**
  基于 fanbox.md Ground Truth 描述，该特征的核心逻辑涉及 `—`。
  Natives 重构为 Rust + Tauri，采用异步非阻塞设计：
  - 所有 I/O 通过 `tokio::fs` 异步执行，不阻塞主线程
  - 状态变更通过 Tauri Event 推送，前端响应式更新
  - 类型安全：Rust enum/struct 编译期消除动态类型错误
  - IPC 安全：Tauri command whitelist，无 HTTP localhost 攻击面

### State Machine
```
Term_RecordingMeta:  idle → processing → (success | error) → idle
```
- `idle`: 初始/空闲状态
- `processing`: 执行业务逻辑
- `success`: 操作成功
- `error`: 操作失败

### Algorithm
```
// Term_RecordingMeta — 增量数据获取（带 offset 缓存 + stale 检测）
async fn fetch_data(db: &SqlitePool) -> Result<DataResult> {
    // 1. 读取持久化缓存 offset
    let cache = sqlx::query_as!(CacheEntry,
        "SELECT file, offset, last_id FROM fetch_cache"
    ).fetch_all(db).await?;
    
    // 2. 增量解析（跳过已处理部分）
    let mut all = Vec::new();
    for entry in walk_files() {
        let offset = cache.get(&entry.path).unwrap_or(0);
        let events = parse_incremental(&entry.path, offset).await?;
        // 持久化新 offset
        sqlx::query!("INSERT OR REPLACE INTO fetch_cache VALUES (?,?,?)",
            entry.path, events.new_offset, events.last_id
        ).execute(db).await?;
        all.extend(events.items);
    }
    
    // 3. Stale 检测
    let result = aggregate(all);
    Ok(if is_stale(&result) { DataResult { ..result, stale: true } } else { result })
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/terminal/recordingmeta.rs` |
| 前端组件 | `src/components/terminal/` |

### Data Model
```typescript
// Term_RecordingMeta — 终端数据模型
interface Term_RecordingMetaParams {
  id: string;
  // 特征专属参数
}
interface Term_RecordingMetaResult {
  success: boolean;
  id: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn recordingmeta(params: ...) -> Result<Term_RecordingMetaResult>
```

### Performance & Security Design
| 维度 | 方案 |
|------|------|
| **异步** | 所有 I/O 通过 tokio async 执行，主线程零阻塞 |
| **内存** | 内存边界控制（大文件截断 / LRU 缓存上限 / 流式处理） |
| **安全** | Tauri IPC whitelist + 路径校验 / sandbox iframe / validName |
| **跨平台** | `#[cfg(target_os)]` 条件编译处理平台差异 |

## 3. Risk Analysis

### Risks
1. **边界条件**: 极端路径/空结果/权限不足

### Mitigation & Runtime Guardrail
- 输入校验前置，拒绝非法路径/空字节/目录穿越
- 所有异步操作超时控制（`tokio::time::timeout`），防死锁
- 单元测试 + 集成测试覆盖边界条件（空目录 / 权限不足 / 符号链接循环 / 并发写入）
- 日志分级输出，关键路径有 error / warn / info 三级追踪
