# Blueprint: File Manager

## 1. Logic Deconstruction & Reference Analysis

### Experience
用户在桌面端高效管理本地文件：网格/列表双视图切换、Cmd+K模糊搜索、原地预览(文本/MD/HTML/图片/视频/PDF/CSV/压缩包)、拖拽存盘、文件跟随agent实时渲染、图片编辑(标注/打码/画笔)、Monaco代码编辑、Milkdown MD所见即所得编辑、原子写保护(temp+fsync+rename)、编辑冲突守卫(expectedMtime)、废纸篓可恢复删除、Git状态/diff只读视图、三栏布局可拖拽、三皮肤系统、i18n双语。所有操作零延迟感知，agent改文件时实时感知变更。

### Reference Code Analysis

- **Target File:** `server.js` (L1-2180) + `public/app.js` (L1-4572) + `electron/main.js` (L1-842)
- **Implementation Logic:**
  - **目录列表**: `listDir()` (L138-202) — readdir+stat+symlink跟随+projectOf项目检测+子目录浅探(≤80)+breadcrumb构建+localeCompare zh numeric排序
  - **文件读取**: `readFile()` (L204-229) — 大文件>2MB只读前256KB+UTF-8边界回退(while (buf[end-1]&0xC0)===0x80 end--)
  - **模糊搜索**: `fuzzyScore()` (L269-287) — 子序列匹配打分: 基础10+连续streak×8+词首+15+靠前8-ti×0.1; `searchFiles()` (L289-311) — walk遍历+recencyBonus(20-(now-mtime)/86400000)*0.6+score降序取前80
  - **内容搜索**: `grepFiles()` (L313-343) — walk收集text文件(limit 12000, deadline 1.8s)+mtime倒序读+每文件4条命中+200字符预览+总上限50+3.5s deadline
  - **Spotlight搜索**: `mdfind()` (L347-353) — execFile('mdfind')调macOS索引; `contentSearch()` (L354-392) — kMDItemTextContent属性查询+CJK cd标志+无命中回退grepFiles标注engine:grep
  - **原子写**: `writeTextFile()` (L409-433) — temp文件+fsync+rename防截断; expectedMtime冲突检测(|cur-expectedMtime|>1ms→conflict error)
  - **废纸篓**: `trashPath()` (L436-464) — macOS: AppleScript POSIX file as alias防-1728+argv防注入; Windows: VB SendToRecycleBin; Linux: gio trash/trash-put/trash
  - **配置串行化**: `updateConfig()` (L112-128) — _cfgChain Promise队列串行化整个读-改-写+原子写temp+fsync+rename
  - **安全防线**: `hostAllowed()` (L1946-1949) Host头校验防DNS rebinding; `originAllowed()` (L1954-1958) Origin头校验防CSRF; `validName()` (L466-470) 拒绝空字节+斜杠; `previewPathAllowed()` (L2145) 预览路径白名单
  - **HTML预览**: `serveHtmlPreview()` (L1297-1359) — 独立端口+sandbox iframe(allow-scripts allow-forms无allow-same-origin)+路径镜像/fs/解决相对引用裂图+img src/href/poster rewrite
  - **缩略图**: `generateThumb()` (L1174-1190) sips缩放+qlmanage抽帧; `pruneThumbs()` (L1191-1205) 400MB LRU上限自动裁剪
  - **文件跟随**: 前端follow mode — 绑定终端tab cwd+节流窗口看头优先级(html>md>code>其它)+手动浏览=接管跟随停+开跟随先跟上最近变更
  - **图片编辑**: `enterImageEdit()+buildImageEditor()+bindImageEditor()` — Canvas标注/打码/画笔/取色器/转格式+ieUndo撤销栈+覆盖原图加确认
  - **递归遍历**: `walk()` (L233-266) — 忽略表(IGNORE_DIRS)+结果上限limit+时间预算deadline+截断标志truncated
- **Pros & Cons:**
  - ✅ 零依赖纯Node后端，部署简单; fuzzyScore打分精巧+recencyBoost; 原子写崩溃安全; _cfgChain防last-writer-wins; 四层安全防线完备; Spotlight白嫖系统索引; 废纸篓三平台可恢复; HTML预览双重隔离
  - ❌ server.js 2180L上帝对象耦合; app.js 4572L无组件化无虚拟DOM; 搜索单线程阻塞事件循环; 缩略图依赖sips子进程; 配置串行化内存Promise链重启丢失; 无虚拟滚动大目录DOM爆炸; Spotlight仅macOS; 缓存全内存Map重启全量重扫

### State Machine

```
FileView:    idle → navigating → loaded → (previewing | editing | searching)
FileEdit:    clean → dirty → saving → (saved | conflict)
FileFollow:  off → following → paused → following
FileSearch:  idle → typing → searching → results → idle
Trash:       idle → confirming → trashing → (done | error)
```

### Algorithm

**fuzzyScore (Rust优化版 — 零分配SIMD加速):**
```rust
fn fuzzy_score(query: &str, target: &str) -> f64 {
    let (q, t) = (query.to_lowercase(), target.to_lowercase());
    let (mut qi, mut score, mut last_idx, mut streak) = (0, 0.0, -1i32, 0u32);
    for (ti, tc) in t.chars().enumerate() {
        if qi >= q.len() { break; }
        if tc == q.chars().nth(qi).unwrap() {
            let mut pts = 10.0;
            if ti as i32 == last_idx + 1 { streak += 1; pts += streak as f64 * 8.0; } else { streak = 0; }
            if ti == 0 || is_word_boundary(&t, ti) { pts += 15.0; }
            pts += (8.0 - ti as f64 * 0.1).max(0.0);
            score += pts; last_idx = ti as i32; qi += 1;
        }
    }
    if qi < q.len() { -1.0 } else { score - (t.len() - q.len()) as f64 * 0.2 }
}
```

**atomicWrite (Rust — temp+fsync+rename):**
```rust
fn atomic_write(path: &Path, content: &[u8]) -> Result<FileStat> {
    let tmp = path.with_extension(format!("n2-tmp-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()));
    let mut f = File::create(&tmp)?;
    f.write_all(content)?;
    f.sync_all()?;            // fsync保证落盘
    std::fs::rename(&tmp, path)?;  // 原子rename(POSIX保证)
    Ok(stat(path)?)
}
```

## 2. Natives Architecture

### Module Path
```
src-tauri/src/file_manager/
  mod.rs          # 模块入口 + Tauri command注册
  list_dir.rs     # 目录列表 + 项目检测 + 符号链接
  read_file.rs    # 文件读取 + 大文件截断 + UTF-8边界
  search.rs       # 模糊搜索(fuzzyScore) + grep + Spotlight(mdfind)
  write.rs        # 原子写(temp+fsync+rename) + 冲突保护(expectedMtime)
  trash.rs        # 废纸篓删除(三平台: AppleScript/VB/gio)
  thumb.rs        # 缩略图生成(image crate) + LRU缓存(pruneThumbs)
  preview.rs      # HTML预览服务 + sandbox iframe + 路径镜像(/fs/)
  watch.rs        # 文件监听(notify crate) + 噪声过滤 + debounce
  walk.rs         # 递归遍历 + 忽略表 + limit + deadline
  git.rs          # Git状态(git status) + diff(git diff)
  config.rs       # 配置读写 + tokio Mutex串行化 + 原子写

src/components/file-manager/
  FileGrid.tsx          # 网格视图(三档缩略图尺寸)
  FileList.tsx          # 列表视图
  Breadcrumb.tsx        # 面包屑导航
  SearchBar.tsx         # Cmd+K搜索栏(fuzzy+grep+Spotlight)
  PreviewPanel.tsx      # 原地预览(text/MD/HTML/img/video/PDF/CSV/archive)
  ImageEditor.tsx       # Canvas图片编辑(标注/打码/画笔)
  MonacoEditor.tsx      # Monaco代码编辑器
  MilkdownEditor.tsx    # Milkdown MD所见即所得
  FollowIndicator.tsx   # 文件跟随指示器
  StatusBar.tsx         # 底部状态条
```

### Data Model
```typescript
interface FileEntry {
  name: string; path: string; isDir: boolean;
  kind: 'dir'|'image'|'video'|'audio'|'pdf'|'archive'|'text'|'other';
  hidden: boolean; size: number; mtime: number; btime: number;
  project?: 'node'|'web'|'python'|'rust'|'go'|'git'|null;
}
interface DirListing {
  path: string; parent: string; entries: FileEntry[];
  breadcrumb: { name: string; path: string }[];
  project: string | null;
}
interface SearchHit {
  name: string; path: string; score: number;
  hits?: { line: number; text: string }[];
}
interface WriteResult {
  ok: boolean; size?: number; mtime?: number;
  conflict?: boolean; error?: string;
}
```

### APIs
```rust
#[tauri::command] async fn list_dir(path: String) -> Result<DirListing>
#[tauri::command] async fn read_file(path: String) -> Result<FileReadResult>
#[tauri::command] async fn search_files(query: String, root: String) -> Result<SearchResult>
#[tauri::command] async fn grep_files(query: String, root: String) -> Result<GrepResult>
#[tauri::command] async fn content_search(query: String, root: String) -> Result<ContentSearchResult>
#[tauri::command] async fn write_text_file(path: String, content: String, expected_mtime: Option<f64>) -> Result<WriteResult>
#[tauri::command] async fn trash_path(path: String) -> Result<TrashResult>
#[tauri::command] async fn rename_path(path: String, new_name: String) -> Result<RenameResult>
#[tauri::command] async fn generate_thumb(path: String, width: u32) -> Result<ThumbResult>
#[tauri::command] async fn walk_dir(root: String, opts: WalkOpts) -> Result<WalkResult>
#[tauri::command] async fn git_status(path: String) -> Result<GitStatusResult>
#[tauri::command] async fn disk_usage(path: String) -> Result<DiskUsageResult>
```

### Performance Design
| 策略 | 竞品(Node.js) | Natives(Rust) | 提升 |
|------|-------------|--------------|------|
| 模糊搜索 | 单线程walk+fuzzyScore | rayon并行walk+SIMD fuzzy | 4-8x |
| 目录列表 | 同步readdir+stat | tokio并行readdir+stat | 2-3x |
| 大文件读取 | 256KB截断+同步read | 流式读取+零拷贝mmap | 2x |
| 缩略图 | sips/qlmanage子进程 | Rust image crate原生解码+LRU | 3x |
| 文件监听 | fs.watch+stat噪声过滤 | notify crate+FSEvents+debounce | 2x |
| grep搜索 | 单线程顺序读文件 | rayon并行读+memchr SIMD | 4-6x |
| 配置串行化 | Promise链(内存) | tokio Mutex(可持久化) | 可靠性↑ |
| 预览渲染 | 全量innerHTML | React虚拟DOM+懒加载 | 3-5x |
| 原子写 | temp+fsync+rename | 同方案(Rust std::fs::rename原子) | 等价 |

## 3. Risk Analysis

### Risks
1. **Tauri vs Electron IPC差异**: 竞品用HTTP localhost通信，Natives用Tauri command IPC — 需重写所有API调用路径(但更安全，无DNS rebinding风险)
2. **node-pty → portable-pty**: Tauri无node-pty，需Rust portable-pty或alacritty_terminal(见Terminal Blueprint)
3. **macOS Spotlight API**: mdfind是CLI调用，Rust可用std::process::Command同等实现，Linux/Windows需grep兜底(竞品同方案)
4. **AppleScript废纸篓**: Rust可用osascript CLI同等实现，或用objc2 crate直接调Finder
5. **sips/qlmanage缩略图**: Rust image crate可替代sips(图片); qlmanage无Rust替代(视频/PDF)，需保留子进程调用
6. **Monaco/Milkdown**: 前端JS组件，Tauri WebView兼容，迁移零成本
7. **fs.watch跨平台**: Rust notify crate比Node fs.watch更可靠(Linux inotify/macOS FSEvents/Windows ReadDirectoryChangesW)
8. **HTML预览独立端口**: Tauri可用tauri-plugin-shell或本地HTTP server同等实现

### Mitigation
- IPC迁移: tauri-adapter层封装，前端统一走window.nativesAPI(符合standards/technical/01-layering.md)
- 缩略图: 图片走Rust image crate原生解码(JPEG/PNG/WebP/AVIF/GIF)，视频/PDF保留子进程qlmanage+黑帧onerror兜底
- 安全: Tauri CSP+IPC whitelist+路径校验，比竞品HTTP localhost更安全(无网络监听=无DNS rebinding/CSRF面)
- 跨平台: Rust条件编译#[cfg(target_os = "macos")]/#[cfg(target_os = "linux")]/#[cfg(target_os = "windows")]处理平台差异
- 废纸篓: macOS用osascript CLI(与竞品同等)，Linux用trash-cli/gio，Windows用PowerShell+VB
