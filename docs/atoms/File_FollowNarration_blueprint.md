# Blueprint: File_FollowNarration (Repaired — Feature-Specific)

## 1. Logic Deconstruction & Reference Analysis

### Experience
文件跟随的过程旁白，从绑定终端tab输出尾巴提炼agent动作(认Claude Code的Update/Bash/Read/Grep/Web Search)，翻成写X/跑Y/搜Z实时显示，忙时脉冲点闲时静止点。

### Reference Code Analysis
- **Target File:** `src-tauri/src/file_manager/follownarration.rs`
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
File_FollowNarration:  idle → typing → (debounce) → searching → (results | no_results) → idle
```
- `idle`: 搜索栏空闲
- `typing`: 用户输入
- `debounce`: 节流等待（防高频触发）
- `searching`: 执行搜索（fuzzy/grep/Spotlight）
- `results` / `no_results`: 结果返回或空结果

### Algorithm
```
// File_FollowNarration — 异步搜索（fuzzyScore / grep / Spotlight）
async fn search(query: &str, root: &str, mode: SearchMode) -> Result<SearchResult> {
    // 1. 子序列模糊匹配打分
    let candidates = walk_candidates(root, WalkOpts { limit: 12000, deadline: 1.8 }).await?;
    let scored: Vec<_> = candidates.par_iter()
        .map(|f| (f, fuzzy_score(query, &f.name)))
        .filter(|(_, s)| *s >= 0.0)
        .sorted_by(|a, b| b.1.partial_cmp(&a.1))
        .take(80)
        .collect();
    
    // 2. recency boost
    let now = Utc::now().timestamp();
    Ok(scored.into_iter().map(|(f, score)| SearchHit {
        name: f.name, path: f.path,
        score: score + recency_bonus(f.mtime, now),
    }).collect())
}
```

## 2. Natives Architecture

### Module Path
| 层 | 路径 |
|---|------|
| Rust 后端 | `src-tauri/src/file_manager/follownarration.rs` |
| 前端组件 | `src/components/files/` |

### Data Model
```typescript
// File_FollowNarration — 文件管理器数据模型
interface File_FollowNarrationParams {
  path: string;
  // 特征专属参数
}
interface File_FollowNarrationResult {
  success: boolean;
  path: string;
  error?: string;
}
```

### APIs
```rust
#[tauri::command]
async fn follownarration(params: ...) -> Result<File_FollowNarrationResult>
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
