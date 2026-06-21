# Blueprint: Skills

## 1. Logic Deconstruction & Reference Analysis

### Experience
透视所有已安装AI agent技能(skills)：五源聚合扫描(~/.claude/skills + ~/.codex/skills + ~/.agents/skills + Claude插件 + 项目级.claude/skills)，健康检查(缺frontmatter标红/超长description标黄/残留文件标灰)，触发统计(45天Claude+Codex会话日志增量解析)，Context预算条(15000字符预警)，启停开关(移入_disabled/可逆)，终端调用(拖进终端注入/skill-name)，卸载(移入废纸篓+validateSkillDir安全护栏)。

### Reference Code Analysis

- **Target File:** `server.js` (L1640-1940) + `public/app.js` skills UI
- **Implementation Logic:**
  - **五源扫描**: `scanSkillRoot()` (L1660-1708) — 递归遍历+软链跟随(isSymbolicLink→stat解析真实类型); _disabled/子目录标记disabled=true; 跳过.开头/_archive/_backups; 残留文件(非目录非.md)标residue
  - **frontmatter解析**: `skillFrontmatter()` (L1649-1658) — 正则/^---\s*\r?\n([\s\S]*?)\r?\n---/提取; description处理块标量(>-)和引号包裹; 超240字符截断显示
  - **健康检查**: skillsData()内 — description超1536字符截断线标黄(后段触发词模型看不见); 缺frontmatter标红; 缺SKILL.md标红; zip等残留物标灰; 仅看问题过滤
  - **触发统计**: `claudeSkillEvents()` (L1712-1770) — 增量解析Claude Code jsonl: offset追踪+lastMsgId去重; Skill tool_use(模型自动触发)+<command-name>(用户手动调用); `codexSkillEvents()` (L1771-1810) Codex rollout按会话去重; 45天cutoff; claudeSkillStatCache Map缓存增量状态
  - **启停开关**: `skillToggle()` (L1893-1919) — 停用=移入_disabled/子目录(立即对模型不可见、不删文件、可逆); 不用官方skillOverrides(已知bug claude-code#50631); 软链接型skill先解析绝对目标再迁移避免相对链接断链
  - **卸载**: `skillTrash()` (L1927) — 移到系统废纸篓(trashPath)可恢复; `validateSkillDir()` (L1885) 只允许动最近一次扫描出来的skill目录杜绝任意路径删除
  - **Context预算**: SKILL_BUDGET_CHARS=15000 — 全局常驻description总量vs预算估算线; 超限红色警示; 项目级skill不计入常驻预算
  - **跨来源副本**: skillsData()内 — 同名skill出现在几处标copies=N处副本
- **Pros & Cons:**
  - ✅ 五源全覆盖+软链跟随; 增量解析避免全量重读; _disabled/方案可靠可逆绕过官方bug; validateSkillDir安全护栏; 预警机制防context溢出
  - ❌ 每次全量扫描无增量(30s缓存); 正则解析YAML不严格; 缓存是内存Map重启全量重扫; _disabled/跨设备rename可能失败; 15000预算是社区估算非官方确认

### State Machine

```
SkillScan:    idle → scanning → scanned → (health_checking → health_checked)
SkillToggle:  enabled → disabling → disabled → enabling → enabled
SkillTrash:   idle → confirming → trashing → done
```

### Algorithm

**scanSkillRoot (Rust — 并行扫描+SQLite增量缓存):**
```rust
fn scan_skill_root(root: &Path, source: &str, label: &str, out: &mut Vec<SkillItem>, disabled: bool) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "_archive" || name == "_backups" { continue; }
        let fp = entry.path();
        if name == "_disabled" {
            if fp.is_dir() && !disabled { scan_skill_root(&fp, source, label, out, true)?; }
            continue;
        }
        let mut is_dir = fp.is_dir();
        if entry.file_type()?.is_symlink() { is_dir = fs::metadata(&fp)?.is_dir(); }  // 软链跟随
        if !is_dir { out.push(SkillItem::residue(&name, &fp, source, label)); continue; }
        let mut item = SkillItem::new(&name, &fp, source, label, disabled);
        let sm = fp.join("SKILL.md");
        if sm.exists() {
            let head = read_head(&sm, 32768)?;
            if let Some(fm) = parse_frontmatter(&head) {
                item.desc = fm.desc.chars().take(240).collect();
                item.desc_len = fm.desc.len();
                if fm.desc.len() > SKILL_DESC_CUT { item.issues.push(format!("desc {} > {} truncation", fm.desc.len(), SKILL_DESC_CUT)); }
            } else { item.issues.push("lacks frontmatter description".into()); }
        } else { item.residue = true; item.issues.push("Missing SKILL.md".into()); }
        out.push(item);
    }
    Ok(())
}
```

## 2. Natives Architecture

### Module Path
```
src-tauri/src/skills/
  mod.rs              # 模块入口 + Tauri command注册
  scan.rs             # 五源聚合扫描 + 软链跟随
  frontmatter.rs      # YAML frontmatter解析(serde_yaml + 正则fallback)
  health.rs           # 健康检查(红黄绿标注)
  trigger_stats.rs    # 触发统计(增量解析jsonl + SQLite缓存offset/lastMsgId)
  toggle.rs           # 启停开关(_disabled/迁移 + 跨设备copy+delete fallback)
  trash.rs            # 卸载(废纸篓 + validateSkillDir)
  budget.rs           # Context预算计算(SKILL_BUDGET_CHARS)

src/components/skills/
  SkillsPanel.tsx
  SkillCard.tsx
  SkillHealthBadge.tsx
  SkillBudgetBar.tsx
  SkillTriggerChart.tsx
```

### Data Model
```typescript
interface SkillItem {
  name: string; dir: string; source: string; label: string;
  disabled: boolean; residue: boolean;
  desc: string; descLen: number; mtime: number;
  issues: string[]; copies?: number;
  hits?: number; lastHit?: number;
}
interface SkillsOverview {
  total: number; unique: number; active: number;
  totalTriggers: number; dusty: number; issues: number;
  budgetUsed: number; budgetTotal: number;
}
```

### APIs
```rust
#[tauri::command] async fn skills_data() -> Result<SkillsData>
#[tauri::command] async fn skill_toggle(dir: String, enable: bool) -> Result<()>
#[tauri::command] async fn skill_trash(dir: String) -> Result<TrashResult>
#[tauri::command] async fn validate_skill_dir(dir: String) -> Result<bool>
```

### Performance Design
| 策略 | 竞品(Node.js) | Natives(Rust) | 提升 |
|------|-------------|--------------|------|
| 扫描 | 同步readdir全量 | Rust并行扫描+SQLite增量缓存 | 2x启动, 10x+重启 |
| frontmatter | 正则解析 | serde_yaml严格解析+正则fallback | 可靠性↑ |
| 触发统计 | 内存Map缓存 | SQLite持久化offset+lastMsgId | 10x+重启恢复 |
| 预算计算 | 遍历求和 | 预计算+增量更新 | 2x |

## 3. Risk Analysis

### Risks
1. **YAML解析**: 竞品用正则, Natives用serde_yaml更严格但复杂frontmatter可能不兼容
2. **软链跨设备**: _disabled/迁移跨挂载点rename失败(不同st_dev)
3. **触发统计缓存**: 竞品用内存Map, Natives用SQLite持久化但增加复杂度
4. **Claude/Codex日志格式变化**: jsonl格式非公开API, 版本升级可能破坏解析

### Mitigation
- YAML: serde_yaml为主, 正则fallback处理非标准frontmatter
- 跨设备: 检测st_dev, 不同则copy+delete替代rename
- 缓存: SQLite存储offset/lastMsgId, 进程重启无需全量重扫
- 日志格式: 版本检测+graceful degradation(解析失败跳过该行)
