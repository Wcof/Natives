# Blueprint: Agents Usage

## 1. Logic Deconstruction & Reference Analysis

### Experience
实时监控AI agent(Claude Code/Codex)的token消耗与配额使用情况：近5h/今日/本周三档聚合，官方限额进度条(5h窗口+7天配额)，多模型同屏对比，自动刷新(60s)，数据来源指示(本地统计vs官方限额)，Codex配额过期守卫，OAuth凭证安全获取。

### Reference Code Analysis

- **Target File:** `server.js` (L1380-1566) + `public/app.js` usage UI
- **Implementation Logic:**
  - **Claude token统计**: `parseClaudeFile()` (L1385-1415) — 增量解析~/.claude/projects下jsonl: offset追踪+lastMsgId去重(同一消息多行落盘); 文件截断重写自动重置offset=0; 末尾不完整行留给下一轮(lastNL); `claudeUsage()` (L1417-1445) — 分近5h/今日/本周三档聚合(input/output/cacheRead/cacheCreate)
  - **Claude文件缓存**: `claudeFileCache` Map (L1382) — file→{offset, lastMsgId, events[]}; stat.size<offset重置; 过期文件自动出缓存
  - **Codex用量**: `codexUsage()` (L1448-1498) — 读~/.codex/sessions下rollout jsonl尾部抓最后一条带rate_limits的token_count(官方配额快照); 按mtime倒序读最新10个文件; 从尾部读262144字节找rate_limits行
  - **Codex过期守卫**: codexUsage()内 (L1482-1493) — capturedAt之后窗口重置时间(resetsAt/resets_in_seconds/window_minutes)已过则usedPercent归零+stale=true; 避免21小时前57%失真数据
  - **OAuth凭证**: `claudeOAuthToken()` (L1505-1526) — macOS从Keychain读(security find-generic-password -s claude -a oauth -w); 其他平台读~/.claude/.credentials.json; 验expiresAt过期
  - **官方限额**: `claudeOfficialLimits()` (L1541-1566) — 用OAuth token调api.anthropic.com/api/oauth/usage; 返回5h窗口+7天配额百分比与重置时间; 走系统curl而非Node https(TLS指纹拦截); token经stdin传入不暴露在进程列表
  - **系统代理**: `curlSysProxyLine()` (L1526-1541) — 打包App从Finder启动无shell代理变量; 读macOS系统代理(scutil --proxy)兜底; 支持HTTPS/HTTP/SOCKS5
  - **TLS指纹绕行**: 调用官方限额接口走系统curl — 该接口按TLS指纹拦(同样请求头curl能200、Node直接403)
- **Pros & Cons:**
  - ✅ 增量解析高效(offset+lastMsgId去重精确); 截断重写自动重置; Codex配额过期守卫防失真; Keychain安全存储; curl绕过TLS指纹+token不暴露在ps
  - ❌ 缓存内存Map重启全量重扫8天日志; Codex只能拿最新快照无历史趋势; Keychain需用户授权; credentials.json明文; curl依赖系统已安装; scutil仅macOS

### State Machine

```
UsageFetch:   idle → fetching → (success | stale | error) → idle
OAuthToken:   valid → expired → refreshing → (valid | error)
ProxyDetect:  none → detecting → (found | not_found) → none
```

### Algorithm

**claudeUsage (Rust + SQLite持久化缓存):**
```rust
async fn claude_usage(db: &SqlitePool) -> Result<ClaudeUsageResult> {
    let cutoff = Utc::now() - Duration::days(8);
    let claude_proj = dirs::home_dir().unwrap().join(".claude/projects");
    // SQLite持久化offset+lastMsgId, 进程重启无需全量重扫
    let cached: HashMap<String, FileCacheEntry> = sqlx::query_as!(
        FileCacheEntry, "SELECT file_path, offset, last_msg_id FROM claude_file_cache"
    ).fetch_all(db).await?.into_iter().map(|e| (e.file_path.clone(), e)).collect();
    let mut all_events = Vec::new();
    for entry in walk_jsonl(&claude_proj, cutoff).await? {
        let cache = cached.get(&entry.fp);
        let events = parse_claude_file(&entry.fp, cache).await?;
        sqlx::query!("INSERT OR REPLACE INTO claude_file_cache (file_path, offset, last_msg_id) VALUES (?, ?, ?)",
            entry.fp, events.new_offset, events.last_msg_id).execute(db).await?;
        all_events.extend(events.items);
    }
    aggregate_usage(&all_events)  // 3 buckets: last5h, today, week
}
```

**OAuth Token (Rust keyring — macOS Keychain / 其他credentials.json):**
```rust
fn claude_oauth_token() -> Result<OAuthToken> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("security")
            .args(["find-generic-password", "-s", "claude", "-a", "oauth", "-w"])
            .output()?;
        let token = String::from_utf8(output.stdout)?.trim().to_string();
        Ok(OAuthToken { token, ..parse_expires(&token)? })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let creds: Value = serde_json::from_str(&fs::read_to_string(
            dirs::home_dir().unwrap().join(".claude/.credentials.json"))?)?;
        Ok(OAuthToken::from_json(&creds)?)
    }
}
```

## 2. Natives Architecture

### Module Path
```
src-tauri/src/agents_usage/
  mod.rs              # 模块入口 + Tauri command注册
  claude_usage.rs     # Claude token统计 + 增量解析 + SQLite缓存
  codex_usage.rs      # Codex用量 + 配额快照 + 过期守卫(stale)
  oauth.rs            # OAuth token获取(Keychain/credentials.json + expiresAt)
  official_limits.rs  # 官方限额查询(curl + TLS指纹绕行 + token经stdin)
  proxy_detect.rs     # 系统代理检测(scutil macOS / env Linux / WinHTTP Windows)
  aggregate.rs        # 三档聚合(5h/today/week)

src/components/agents-usage/
  UsagePanel.tsx
  ClaudeUsageCard.tsx
  CodexUsageCard.tsx
  UsageProgressBar.tsx
  MultiModelView.tsx
```

### Data Model
```typescript
interface ClaudeUsage {
  last5h: UsageBucket; today: UsageBucket; week: UsageBucket;
  officialLimits?: { primary: RateLimit; secondary: RateLimit };
}
interface UsageBucket {
  total: number; input: number; output: number;
  cacheRead: number; cacheCreate: number; msgs: number;
}
interface RateLimit {
  usedPercent: number; windowMinutes: number;
  resetsAt: number; stale: boolean;
}
interface CodexUsage {
  planType: string; capturedAt: number;
  primary: RateLimit | null; secondary: RateLimit | null;
}
```

### APIs
```rust
#[tauri::command] async fn claude_usage() -> Result<Option<ClaudeUsage>>
#[tauri::command] async fn codex_usage() -> Result<Option<CodexUsage>>
#[tauri::command] async fn agent_usage() -> Result<AgentUsageData>
```

### Performance Design
| 策略 | 竞品(Node.js) | Natives(Rust) | 提升 |
|------|-------------|--------------|------|
| 增量解析 | 内存Map缓存offset | SQLite持久化offset+lastMsgId | 10x+重启恢复 |
| 聚合计算 | 每次全量遍历events | 预聚合+增量更新 | 2x |
| OAuth token | security CLI子进程 | keyring crate原生调用 | 等价 |
| 官方限额 | curl子进程(TLS指纹) | 保留curl(reqwest同样被拦) | 等价 |
| 代理检测 | scutil CLI | Rust system-proxy crate | 等价 |
| 自动刷新 | setInterval 60s | tokio interval + 前端Event | 等价 |

## 3. Risk Analysis

### Risks
1. **OAuth token存储**: macOS Keychain需用户授权; credentials.json明文不安全
2. **TLS指纹拦截**: Anthropic API按TLS指纹拦, Rust reqwest同样被403, 必须保留curl绕行
3. **jsonl格式非公开API**: Claude/Codex日志格式可能随版本变化
4. **Codex配额快照**: 只能拿最新一条rate_limits, 无历史趋势
5. **跨平台代理检测**: scutil仅macOS, Linux读env/NetworkManager, Windows读WinHTTP

### Mitigation
- OAuth: macOS用keyring crate(Keychain Services API), 其他平台用credentials.json+文件权限0600
- TLS: 保留curl子进程调用, token经stdin传入不暴露在ps
- jsonl: 版本检测+graceful degradation, 解析失败跳过该行
- 代理: 条件编译#[cfg(target_os)], macOS scutil, Linux env/NM, Windows WinHTTP
- 安全: Tauri CSP限制出网请求仅api.anthropic.com
