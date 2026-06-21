# Audit Report: Search Subsystem (FuzzyScore / Walk / Grep / Spotlight)
**Audit Date:** 2026-06-21 (UTC+8)
**Base Reference:** Docs/fanbox.md — File Search & Content Search modules
**Target Audited Code:** `src-tauri/src/search.rs` (229 lines)
**Competitor Reference:** `/References/fanbox/server.js` lines 233-392

## 1. Executive Summary

Natives2 的搜索子系统实现了 FanBox 的**骨架结构**（BFS 遍历 + mdfind + grep 兜底 + 行级预览），但缺失了 FanBox 搜索体验的**灵魂** — `fuzzyScore` 模糊评分算法。当前搜索是二元匹配（contains → 命中/不命中），而 FanBox 是连续子序列匹配 + 多维权重叠加。这导致搜索结果质量存在数量级差距。

**合规度评估**: 结构覆盖 ~70%，体验覆盖 ~30%。

## 2. Discovered Issues & Gaps

### 🔴 Critical Issues (阻断级 — 必须修复)

#### Issue C-1: fuzzyScore 算法完全缺失
- **Location:** `search.rs:55-63` — 当前评分逻辑
- **Violation:** `Docs/fanbox.md` 要求的模糊匹配能力
- **Current Code:**
  ```rust
  let mut score = if name_lower == query_lower {
      1.0
  } else if name_lower.starts_with(&query_lower) {
      0.8
  } else {
      0.5
  };
  ```
- **FanBox Reference (server.js:269-287):**
  ```javascript
  function fuzzyScore(query, target) {
    // 子序列匹配 + streak×8 + 词首+15 + 位置衰减(8 - ti*0.1) - 长度惩罚(t-q)*0.2
  }
  ```
- **Impact:** 搜索 "agent" 时，FanBox 能精准排序 `agent-dashboard.tsx` > `AgentCard.tsx` > `page-manager.tsx`（因连续命中+词首加分）。Natives2 三者得分完全相同（都是 contains=0.5）。
- **Severity:** 🔴 Critical — 搜索是用户最高频操作，质量差距直接影响"生产力工具"定位。

#### Issue C-2: 路径评分 (pathBonus) 缺失
- **Location:** `search.rs:77-83` — 只保存文件名到 `text` 字段
- **Violation:** FanBox `searchFiles` (server.js:297): `const pathBonus = fuzzyScore(q, f.path) > 0 ? 3 : 0`
- **Impact:** 用户搜索 "src/components" 时，FanBox 能匹配路径中包含这些片段的文件，Natives2 完全忽略路径。
- **Severity:** 🔴 Critical — 中文用户常用路径搜索定位文件。

#### Issue C-3: 目录加权 (dirBonus) 缺失
- **Location:** `search.rs:37-86` — 遍历逻辑只处理文件，目录仅入队
- **Violation:** FanBox `searchFiles` (server.js:307): `onDir: (f) => scoreInto(f, 6)` — 目录额外+6分
- **Impact:** "vibe coding 一下午起十个项目" 场景下，用户最常搜索的是项目目录本身。Natives2 搜索结果中目录永远不会出现。
- **Severity:** 🔴 Critical — 破坏了 FanBox "目录优先" 的设计哲学。

#### Issue C-4: Spotlight 查询语法差异
- **Location:** `search.rs:164` — `mdfind -onlyin root query`
- **Violation:** FanBox (server.js:360): `mdfind -onlyin root '(kMDItemTextContent == "*esc*"cd) || (kMDItemDisplayName == "*esc*"cd)'`
- **Impact:** FanBox 使用 Spotlight 属性查询语法（`kMDItemTextContent`），支持 PDF/DOCX 全文搜索和 OCR 文字。Natives2 传裸 query，仅匹配文件名元数据。
- **Severity:** 🔴 Critical — Spotlight 的核心价值（PDF/DOCX/OCR 全文搜索）被完全浪费。

### 🟡 Minor Gaps & Architectural Drift

#### Gap M-1: grep 子进程硬依赖
- **Location:** `search.rs:112-134`
- **Current:** 直接调用 `rg` 或 `grep` 子进程，无错误处理
- **FanBox:** 内置 JS 遍历 + 逐文件读取 (server.js:313-343)
- **Suggestion:** 添加 `rg`/`grep` 不可用时的 fallback — 使用 Rust 内置文件遍历 + `std::fs::read_to_string` 逐行匹配。当前代码在 `rg` 不存在时会静默失败（`which("rg")` 返回 false，切到 `grep`，但 `grep` 也可能不存在）。

#### Gap M-2: grep 结果无排序
- **Location:** `search.rs:141-156`
- **Current:** 结果按 `rg`/`grep` 输出顺序（文件系统顺序），无 mtime 排序
- **FanBox (server.js:325):** `files.sort((a, b) => b.mtime - a.mtime)` — 按修改时间倒序
- **Suggestion:** 在 `SearchResult` 中增加 `mtime` 字段，grep 结果按 mtime 排序。"我最近写过那句话" 的文件应优先命中。

#### Gap M-3: Spotlight 行级预览限制
- **Location:** `search.rs:192-217`
- **Current:** 预览所有小文本文件（< 256KB），无数量限制
- **FanBox (server.js:378-381):** `if (read >= 12) break` — 最多预览 12 个文件
- **Suggestion:** 添加预览文件数量上限（12-15个），避免大结果集时的性能问题。

#### Gap M-4: mdfind 超时缺失
- **Location:** `search.rs:164-167`
- **Current:** `mdfind` 无超时控制
- **FanBox (server.js:349):** `execFile('mdfind', args, { timeout: 6000, maxBuffer: 8 * 1024 * 1024 })`
- **Suggestion:** 使用 `std::process::Command` 的 `kill_on_drop` + 超时包装，或使用 `tokio::time::timeout` 包装异步调用。

#### Gap M-5: walk 中 lstat 错误静默忽略
- **Location:** `search.rs:41-44` — `std::fs::read_dir` 失败时 continue
- **FanBox (server.js:243):** 同样 `catch { continue }`，但 FanBox 额外处理了符号链接（`d.isDirectory()` 检查）
- **Suggestion:** 当前 `entry_path.is_dir()` 对符号链接可能产生意外行为。考虑使用 `entry.file_type()?.is_dir()` 代替。

#### Gap M-6: mdfind 结果无 mtime 排序
- **Location:** `search.rs:170-179`
- **Current:** mdfind 结果按系统返回顺序
- **FanBox (server.js:375):** `results.sort((a, b) => b.mtime - a.mtime)` — 按修改时间倒序
- **Suggestion:** mdfind 结果应获取 metadata 后按 mtime 排序。

#### Gap M-7: ignore_dirs 不一致
- **Location:** `search.rs:30-35`
- **Current:** `[".git", ".next", ".cache", "dist", "out", "build", ".vscode", ".idea"]`
- **FanBox (IGNORE_DIRS):** `["node_modules", ".git", "Library", "Applications", ".Trash", ...]`（更广泛）
- **Suggestion:** 统一 ignore_dirs 列表，至少添加 `node_modules`（当前列表中缺失！）。

**Wait — 重新检查**: `search.rs:30` 确实包含 `"node_modules"`。但 FanBox 的 IGNORE_DIRS 更广泛，包含系统目录（Library, Applications, .Trash）。Natives2 作为 Tauri 应用，用户通常指定项目根目录，当前列表已足够。

#### Gap M-8: search_grep 无 max_results 传递
- **Location:** `search.rs:118` — `max_results` 用于 `--max-count`，但这是每文件的匹配数上限，不是总结果数上限
- **Impact:** rg 可能返回远超 `max_results` 的结果
- **Suggestion:** 在 `stdout.lines().take(max_results)` 已有限制（line 141），但应在 rg 层面也限制（`--max-count` 是每文件限制，需用 `--max-filesize` 或后处理）。

## 3. Deterministic State Machine Review

- [x] 状态转移是否覆盖完全？ — **Yes**，BFS 遍历有 deadline + limit 双控
- [x] 是否存在死锁或未定义状态？ — **No**，同步 `std::fs` 操作，无异步竞争

**注意**: 当前 `search.rs` 使用同步 `std::fs`，而 FanBox 使用异步 `fsp.readdir`。在 Tauri 的命令线程中这是可接受的（Tauri 命令在独立线程池中执行），但如果未来迁移到 async/await 模式，需要改为 `tokio::fs`。

## 4. Refactoring & Repair Action Items

- [ ] **Action 1 (Critical):** 在 `search.rs` 中添加 `fuzzy_score(query, target) -> f64` 函数，移植 FanBox 算法（子序列匹配 + streak×8 + 词首+15 + 位置衰减 - 长度惩罚）。替换当前三档硬编码评分。
  ```rust
  fn fuzzy_score(query: &str, target: &str) -> f64 {
      let q: Vec<char> = query.to_lowercase().chars().collect();
      let t: Vec<char> = target.to_lowercase().chars().collect();
      let (mut qi, mut score, mut last_idx, mut streak) = (0, 0.0, -1i64, 0u32);
      for (ti, &tc) in t.iter().enumerate() {
          if qi >= q.len() { break; }
          if tc == q[qi] {
              let mut pts = 10.0;
              if ti as i64 == last_idx + 1 { streak += 1; pts += streak as f64 * 8.0; }
              else { streak = 0; }
              if ti == 0 || ['/', '_', '-', '.', ' '].contains(&t[ti - 1]) { pts += 15.0; }
              pts += (8.0 - ti as f64 * 0.1).max(0.0);
              score += pts;
              last_idx = ti as i64;
              qi += 1;
          }
      }
      if qi < q.len() { return -1.0; }
      score -= (t.len() - q.len()) as f64 * 0.2;
      score
  }
  ```

- [ ] **Action 2 (Critical):** 在 `search_files` 中添加 pathBonus（对路径也做 fuzzyScore，命中+3）和 dirBonus（目录额外+6）。
  ```rust
  // 在 BFS 循环中，对目录也执行 fuzzy_score
  if entry_path.is_dir() {
      let dir_score = fuzzy_score(&query_lower, &name_lower);
      if dir_score > 0.0 {
          results.push(SearchResult {
              path: entry_path.to_string_lossy().to_string(),
              line: None,
              text: Some(name.clone()),
              score: Some(dir_score + 6.0 + recency_boost),
          });
      }
      queue.push_back(entry_path);
  } else {
      let name_score = fuzzy_score(&query_lower, &name_lower);
      if name_score > 0.0 {
          let path_score = if fuzzy_score(&query_lower, &entry_path.to_string_lossy().to_lowercase()) > 0.0 { 3.0 } else { 0.0 };
          // ...
      }
  }
  ```

- [ ] **Action 3 (Critical):** 修复 `search_spotlight` 的 mdfind 查询语法，使用 `kMDItemTextContent` 属性查询。
  ```rust
  let esc = query.replace(|c: char| c == '\\' || c == '"' || c == '*', "");
  let mdfind_query = format!(
      "(kMDItemTextContent == \"*{esc}*\"cd) || (kMDItemDisplayName == \"*{esc}*\"cd)"
  );
  let output = std::process::Command::new("mdfind")
      .args(["-onlyin", root, &mdfind_query])
      .output();
  ```

- [ ] **Action 4 (Minor):** 为 `search_grep` 添加内置 Rust fallback — 当 `rg` 和 `grep` 都不可用时，使用 `walk` + `read_to_string` 逐行匹配。

- [ ] **Action 5 (Minor):** 为 mdfind 添加超时包装（6秒）。

- [ ] **Action 6 (Minor):** Spotlight 结果按 mtime 排序，行级预览限制最多 12 个文件。

---

## Appendix: FanBox vs Natives2 Search Feature Matrix

| Feature | FanBox | Natives2 | Gap |
|---------|--------|----------|-----|
| Fuzzy subsequence scoring | ✅ 20-line algorithm | ❌ 3-tier hardcoded | 🔴 Critical |
| Path bonus (fuzzyScore on path) | ✅ +3 points | ❌ missing | 🔴 Critical |
| Directory bonus | ✅ +6 points | ❌ directories excluded | 🔴 Critical |
| Recency boost (mtime) | ✅ `(20 - days) * 0.6` | ✅ `0.3 * (1/(1+h/24))` | ✅ Equivalent |
| BFS walk with deadline+limit | ✅ | ✅ | ✅ Equivalent |
| Spotlight kMDItemTextContent | ✅ Property query | ❌ Bare query | 🔴 Critical |
| Spotlight grep fallback | ✅ | ✅ | ✅ Equivalent |
| Line-level preview | ✅ (12 files) | ✅ (unlimited) | 🟡 Minor |
| mdfind timeout | ✅ 6s | ❌ No timeout | 🟡 Minor |
| mdfind result mtime sort | ✅ | ❌ | 🟡 Minor |
| grep internal fallback | ✅ JS-based | ❌ Requires rg/grep | 🟡 Minor |
| grep result mtime sort | ✅ | ❌ | 🟡 Minor |
