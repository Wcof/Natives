# FanBox 原子级子功能拆解清单

> 拆解自 fanbox v1.13.0 代码库（server.js 2180行 / electron/main.js 869行 / public/app.js 333符号 / 全量文档）
> 四大模块：File Manager / Terminal / Skills / Agents Usage

---

## File Manager（文件管理器）

File_GridListView: 分析 fanbox 文件管理器的网格/列表双视图切换机制，含状态记忆、缩略图三档尺寸(68/98/132列宽)、同屏文件数最大化策略。

File_BreadcrumbNav: 分析 fanbox 面包屑路径导航实现，逐级可点、当前目录高亮、macOS根路径适配、窄宽时面包屑可滚动让位给操作按钮。

File_FuzzySearch: 分析 fanbox 的Cmd+K全局模糊搜索，子序列+连续/词首/靠前加权打分算法(fuzzyScore:基础10分+连续streak*8+词首+15+靠前8-ti*0.1)，命中字符高亮渲染(hlFuzzy)，全机/当前目录Tab切换。

File_RecencyBoost: 分析 fanbox 搜索中的近期修改加权策略，searchFiles里recencyBonus按mtime计算，grepFiles按mtime倒序读让最近写过那句话的文件优先命中，我刚做的东西浮出。

File_GrepContent: 分析 fanbox 的内容前缀全文搜索(grepFiles)，递归walk遍历带忽略表+结果上限12000+时间预算1.8s，按mtime倒序读文件，每文件最多4条命中+行号+200字符预览，总结果上限50+3.5s deadline。

File_SpotlightSearch: 分析 fanbox 如何白嫖macOS Spotlight索引(mdfind)做内容搜索，kMDItemTextContent属性查询+CJK子串匹配稳+忽略大小写/音调，覆盖全文+PDF/docx+截图OCR文字毫秒级返回。

File_SpotlightFallback: 分析 fanbox Spotlight到grep双引擎降级链路，mdfind不可用或无命中时自动回退grepFiles并标注engine:grep，索引异常时给用户提示引导修复。

File_LargeFileLazy: 分析 fanbox 大文件延迟加载策略，大于2MB文件仅读前256KB加UTF-8边界回退(末尾多字节字符不切坏)，显示截断提示，避免大文件阻塞渲染。

File_ProjectDetect: 分析 fanbox 进入目录时自动识别项目类型(package.json->NODE/index.html->WEB/pyproject->PYTHON/Cargo.toml->RUST/go.mod->GO/.git->GIT)，面包屑旁+子目录卡片显示徽章，大目录(大于80子目录)跳过浅探避免拖慢。

File_RichPreview: 分析 fanbox 原地验货预览面板：文本/代码(hljs语法高亮)、Markdown(marked渲染)、HTML(iframe实时渲染+源码切换)、图片(内嵌大图+棋盘格透明底)、视频/音频(Range流式播放)、PDF(iframe内嵌)、CSV/TSV(表格化渲染)。

File_HeicPreview: 分析 fanbox 的HEIC/HEIF预览实现，serveRaw遇heic用macOS sips转码jpeg并缓存(复用缩略图LRU)，三条路径透明覆盖，零新依赖。

File_HtmlPreviewSandbox: 分析 fanbox HTML预览的安全隔离架构：独立端口(主端口+1)只出文件不挂/api、sandbox iframe+allow-scripts、可读范围收紧主目录挡点目录、路径镜像端点/fs/解决相对引用裂图。

File_HtmlLocalImageFixup: 分析 fanbox HTML预览的本地图片裂图修复，file://绝对URL主动改写、其余绝对路径加载失败时兜底重写到/fs/镜像端点(对本来能加载的相对/远程/data引用零影响)，覆盖src/href/poster和内联style的background url。

File_HtmlFullInteraction: 分析 fanbox HTML预览支持完整交互：点击、键盘、localStorage、fetch、表单、SPA全部能跑，靠隔离预览源做到安全。

File_ArchivePreview: 分析 fanbox 压缩包预览(zip/jar/tar/tgz/gz)，直接读zip中央目录按bit11判断UTF-8否则GBK解码中文文件名，不依赖系统unzip避免转码丢失。

File_ThumbGen: 分析 fanbox 缩略图生成与缓存体系，图片走sips缩放+视频/PDF走qlmanage QuickLook抽帧，400MB LRU上限自动裁剪(pruneThumbs)，透明格式出PNG保留alpha+非透明出JPEG省空间，并发生成去重(thumbInflight Map)。

File_ThumbRetry: 分析 fanbox 缩略图加载失败自动重试再优雅隐藏，Retina全屏截图较大写盘未完成时等文件大小稳定再通知(waitStable轮询)。

File_OneClickOpen: 分析 fanbox 一键多方式打开：默认应用(open/start/xdg-open)、编辑器(code命令未装回退默认)、Finder显示(open -R)、复制路径、系统终端(open -a Terminal)，跨平台三路适配。

File_FavRecent: 分析 fanbox 收藏与最近机制，任意文件/文件夹可收藏侧边栏常驻，最近打开记录、最近修改扫描(mtime倒序上限60条)，持久化到~/.fanbox/config.json。

File_DotFileToggle: 分析 fanbox 显示/隐藏点文件开关，以及隐藏文件(.env/.git配置等)和sqlite临时sidecar在文件跟随/变更收件箱中的噪声过滤。

File_SortFilter: 分析 fanbox 排序(名称/修改时间/大小，文件夹恒在前，localeCompare zh numeric)+当前目录即时筛选(/聚焦)+键盘导航(上下左右选择/回车预览/Cmd回车编辑器打开/Backspace上级)。

File_WalkIgnoreBudget: 分析 fanbox 递归遍历walk()的工程化控制：忽略表(node_modules/.git/dist等)、结果上限limit、时间预算deadline、截断标志truncated前端展示结果可能不完整警告。

File_FsWatch: 分析 fanbox 的fs.watch递归文件监听，主进程转发事件前先stat检查mtime/ctime是否真正变更(丢弃atime/元数据噪声如LaunchServices的lastuseddate)，读文件从此安静，3秒内自己打开触发的伪事件按噪声丢弃。

File_ChangeBadge: 分析 fanbox 文件变更徽标系统，agent改文件即亮改N计数+tooltip定位深层改动，单一清理定时器防泄漏，isNoisyChange过滤二进制/临时文件噪声。

File_FollowMode: 分析 fanbox 文件跟随的核心机制：绑定终端tab(钉死归属)，只跟该项目cwd下写入；节流窗口内看头优先级html/md大于代码大于其它；agent多文件快写不疯狂跳屏；手动浏览/编辑等于接管跟随自动停；开跟随先跟上最近一次变更不用干等下一笔。

File_FollowNarration: 分析 fanbox 文件跟随的过程旁白，从绑定终端tab输出尾巴提炼agent动作(认Claude Code的Update/Bash/Read/Grep/Web Search)，翻成写X/跑Y/搜Z实时显示，忙时脉冲点闲时静止点。

File_FollowLiveRender: 分析 fanbox 跟随时的三种实时渲染：代码逐行高亮本次写入行并平滑滚过(hljs按行切开渲染)；HTML双缓冲(隐藏iframe加载完才换上零白闪)；MD边写边渲染尾部追加贴底滚动。

File_FollowArtifact: 分析 fanbox 跟随产物卡片，二进制/压缩包/安装包不再当文本跟随显示无法预览，改成干净卡片(图标+agent刚生成+大小+在Finder显示)。

File_FollowNoiseFilter: 分析 fanbox 跟随噪声过滤四层：atime/元数据事件stat判否丢弃；隐藏文件+sqlite sidecar+tmp文件统一进噪声名单；跟随范围绑定终端tab的cwd不跟别的agent；节流窗口内看头优先级防低优顶掉高优。

File_ImageEditor: 分析 fanbox 图片编辑器：标注/打码/自由画笔/粗细可视化/原生取色器(自绘圆色点)/转格式/压缩/原生保存，进入编辑即全屏(CSS :has()感知DOM)，覆盖原图加确认(不可逆+有损警告)，ieUndo撤销栈+ieExport导出。

File_MonacoEdit: 分析 fanbox Monaco编辑器集成，语法高亮+Cmd+F查找替换+多光标，停笔自动落盘+撤销/重做图标+实时保存状态(1分钟内显秒/1小时内显分秒/再久显钟点)，未保存守卫堵死Esc旁路。

File_MilkdownEdit: 分析 fanbox Milkdown/Crepe所见即所得MD编辑器，磁盘原文为唯一事实源，语义无损(semanticEqual)才允许富文本否则锁源码灰显切换按钮杜绝静默改写，YAML frontmatter剥离/拼回防丢，外部改盘未脏自动重载脏则保留用户改动。

File_AtomicWrite: 分析 fanbox 原子写与并发覆盖保护：writeTextFile用temp+fsync+rename防截断(写到一半崩溃不留残骸)；updateConfig用_cfgChain Promise队列串行化整个读-改-写杜绝last-writer-wins；写盘失败冒泡。

File_EditConflictGuard: 分析 fanbox 编辑冲突保护，保存时检查expectedMtime，外部改盘或删除则抛conflict错误拒绝盲覆盖，前端弹确认。

File_DragDrop: 分析 fanbox 拖拽存盘体系：Finder文件(有路径copyInto)、截图浮窗(无路径saveTemp到saveInto)、内部img拖入文件区存盘，同名不覆盖仿访达foo 2.png，drop区撑满文件夹高度，dropIn动画+edIn淡入。

File_ScreenshotBus: 分析 fanbox 截图直通车，主进程startShotWatch监听系统截屏落盘(识别截屏命名习惯)，等文件大小稳定再通知(waitStable轮询)，新截图右下角浮出直通卡(到终端喂给agent/收进素材/标注圈重点)，45秒自动消失。

File_ContextNew: 分析 fanbox 右键菜单新建文件/文件夹，双击空白处也可触发，validName校验(小于等于255字符+拒绝斜杠空字节+拒绝点和..)。

File_RenameTrash: 分析 fanbox 重命名(validName校验+同名检测+失败清理tmp)与废纸篓删除(macOS: AppleScript POSIX file as alias强转防-1728/路径走argv防注入/未授权-1743给中文提示；Windows: VB FileSystem SendToRecycleBin；Linux: gio trash/trash-put/trash)，可恢复。

File_DiskUsage: 分析 fanbox 磁盘占用透视，du口径的真实占用条形榜，目录可下钻可回上级，解电脑空间又满了。

File_GitDiff: 分析 fanbox Git状态与diff只读视图，execGit调git命令，gitStatus返回变更文件列表，gitFileDiff返回单文件diff，showDiff渲染高亮。

File_PreviewFullscreen: 分析 fanbox 预览全屏放大，铺满整个窗口盖住文件区/终端/侧边栏，全屏时藏macOS红黄绿系统按钮+关顶栏拖拽区防吞点击，Esc退出，图片全屏居中约束max-height避高图裁顶。

File_LayoutDrag: 分析 fanbox 三栏布局可拖拽调比例(侧栏+文件区+预览+终端)，可折叠面板，布局记忆(window-state.json)，折叠/展开时宽度按比例分配非全甩一侧，animateLayout过渡动画，终端双击顶栏铺满。

File_I18n: 分析 fanbox 国际化实现，集中式翻译层326词条+73插值规则(i18n-dict.js)，默认跟系统语言侧栏可切换，界面/菜单/系统对话框全覆盖，文件名/预览/编辑器/终端用户内容不受影响。

File_ThreeThemes: 分析 fanbox 三皮肤系统(终端Volt荧光绿/暖色档案馆/编辑式粗野)，终端配色与皮肤联动，vibrancy毛玻璃侧栏，SF字体，换肤过渡纳入color避免硬切撕裂，双层阴影+排版呼吸。

File_AutoUpdate: 分析 fanbox 应用内更新检测，启动时查GitHub Releases，2小时周期+窗口聚焦补查(30分钟节流)，API 403降级解析releases页重定向(不占API配额)，失败10分钟重试，渲染层启动时主动补拉防错过。

File_SecurityModel: 分析 fanbox 安全架构：仅127.0.0.1监听、Host头校验防DNS rebinding、Origin头校验防CSRF(POST写操作全验)、空字节路径拒绝(validName)、静态资源目录穿越防护(startsWith+sep)、预览路径白名单(previewPathAllowed)。

File_SidebarTree: 分析 fanbox 侧栏目录树，快速入口/收藏文件夹项带展开箭头逐级懒加载子文件夹(只列文件夹)，点行跳转，当前目录高亮。

File_AgentProjects: 分析 fanbox 侧栏Agent项目列表，自动扫Claude Code和Codex本机会话日志，列出最近30天被coding agent处理过的项目文件夹(agent圆点+活跃时间)，readCwdFromHead从JSONL头部提取真实cwd。

File_StatusBar: 分析 fanbox 底部状态条，当前文件夹基础信息(N项+文件夹/文件数+文件合计大小)，可开合的Agent用量/项目记忆/占用透视入口。

File_AI_Organize: 分析 fanbox AI整理的交互式方案(v2废弃headless v1)，FanBox备料(偏好organize-prefs.md+历史organize-log+约定到organize-brief.md)，一键拉起claude/codex对话式整理，agent先摊方案确认后动手，每批移动写回滚日志，偏好沉淀越用越懂你，删除须逐条点头+进废纸篓不直接rm。

File_ReleaseWizard: 分析 fanbox 发版向导，node项目状态条出现发版——版本号(预填patch+1)、发布说明(预填CHANGELOG Unreleased段)、打包/推送/GitHub Release可勾选，确认后整条命令序列在内嵌终端开跑每步可见可拦。

File_ProjectMemory: 分析 fanbox 项目记忆面板，当前文件夹的agent会话考古——历史会话列表(首条消息当标题)、每次会话改过的文件(点击直达)、触发过的skill，续上一键在内嵌终端claude --resume/codex resume接上当时上下文，数据源本地会话日志按文件缓存增量解析。

File_MoveEntry: 分析 fanbox 文件移动(movePath)和创建(createEntry)操作，移动含冲突检测+同名自动编号，创建支持文件和文件夹两种类型。

File_LocatePath: 分析 fanbox 路径定位系统(locatePath)，支持模糊匹配+多根搜索+tail后缀探测+alt别名+roots多根，statWithTail尝试多种路径变体找到真实文件。

File_CopyImageFile: 分析 fanbox 复制图片/文件到剪贴板(fanboxClipboard)，clip:image调Electron nativeImage写剪贴板，clip:file调shell脚本pbcopy，预览操作区一键复制。

File_SvgIconSystem: 分析 fanbox 全套彩色矢量SVG图标系统，替换所有emoji(品牌logo/面包屑/空状态/收藏星标/预览动作/工具栏/HTML预览按钮)，richIcon按文件类型返回对应SVG，iconColorFor随皮肤变色。

File_VideoThumb: 分析 fanbox 视频缩略图，用qlmanage QuickLook抽帧+黑帧onerror兜底，video标签内嵌播放支持Range流式。

File_SymlinkResolve: 分析 fanbox 符号链接处理，listDir中isSymbolicLink跟随stat解析真实类型，scanSkillRoot中软链跟随解析(skills.sh安装器常用相对路径软链)。

File_ProxyBypass: 分析 fanbox 本地代理旁路，FanBox访问自己localhost后端时显式给loopback加代理旁路(NO_PROXY)，避免clash强制系统代理/企业PAC把本地请求拦成502导致白屏。

File_ConfigSerialization: 分析 fanbox 配置写入串行化，updateConfig用_cfgChain Promise队列串行化整个读-改-写(读也在队列内)，杜绝并发收藏+最近连发导致last-writer-wins丢更新。

---

## Terminal（终端）

Term_PtySpawn: 分析 fanbox 终端核心，Electron主进程node-pty起真PTY，渲染进程xterm.js渲染，IPC通道pty:spawn/input/resize/kill/cwd/proc，每终端独立进程，contextBridge安全桥接。

Term_MultiTab: 分析 fanbox 终端多标签页，每个项目一个agent session常驻不丢，标签栏渲染(renderTabs)+切换(activate)+关闭(closeTab)，关闭时kill PTY防进程泄漏，标签显示cwd目录名+agent状态圆点+忙碌色。

Term_CwdSync: 分析 fanbox 终端cwd与文件树双向联动：点文件夹到终端cd过去(openTermPath)；终端里cd到左侧文件树跟着定位(locateCwd从lsof+cwd文件推断，中文目录名UTF-8 locale+解码兜底)。

Term_PathClickable: 分析 fanbox 终端路径可点击打开，宽进严出策略：含斜杠token候选到stat验证到验证得才划线；裸文件名按扩展名白名单免验证直接划线；全角胶水标点(：；，。、？！)进token切断字符集防误报；目录结尾斜杠也享受同等兜底。

Term_UrlClickable: 分析 fanbox 终端里http(s)链接可直接点击，在系统浏览器打开。

Term_PathDrag: 分析 fanbox 终端文件路径拖进文件区(flingToTerminal反向：文件拖进终端插入路径)，以及skill行拖进终端注入/skill-name。

Term_WebGLRender: 分析 fanbox 终端WebGL渲染加速(@xterm/addon-webgl)，Unicode11宽度修正，高吞吐输出流渲染优化。

Term_LoginShell: 分析 fanbox 终端用login shell启动(zsh -l)，读取~/.zprofile/~/.zlogin里配的PATH，解决GUI app找不到claude命令问题。

Term_NerdFont: 分析 fanbox 终端字体优先使用系统已装Nerd Font(JetBrainsMono/MesloLGS/FiraCode/Hack/Symbols Nerd Font)，Starship/powerline主题图标正常显示不再是方块tofu。

Term_CapsLockPatch: 分析 fanbox xterm vendor补丁，CapsLock(20)在composition中触发提前提交导致双写，补丁加20===e.keyCode豁免，predist守卫脚本check:vendor-patch防补丁静默丢失。

Term_OptClickSelect: 分析 fanbox 终端macOptionClickForcesSelection配置，TUI开启鼠标上报后按住Option拖拽强制选中复制(iTerm/VS Code终端同款约定)。

Term_Recording: 分析 fanbox 终端录像回放(黑匣子)，常开录制把PTY字节流旁路成asciinema v2 .cast(异步写盘失败静默自废对终端零侵入)，recStart/recEvent/recStop事件记录。

Term_RecordingPlayback: 分析 fanbox 录像回放，用与live终端完全相同的xterm配置逐像素一致，时间压缩压等待不压输出(agent思考长静默封顶，流式节奏原样保留)，2小时可压到约1分钟。

Term_RecordingExport: 分析 fanbox 录像导出，本机ffmpeg转MP4/GIF，检测不到ffmpeg优雅退回WebM(asciinema2png渲染+ffmpeg编码)。

Term_AgentLaunch: 分析 fanbox 终端Agent启动按钮(Claude Code/Codex)，空闲shell就地启动，正跑任务则新开标签，launchAgent+runInDir按cwd启动。

Term_AgentRespawn: 分析 fanbox 终端respawn机制，关闭标签后重新打开复用会话目录+reset，不丢上下文。

Term_AgentStatus: 分析 fanbox 终端Agent忙碌/空闲检测，ensureStatusTick定时扫描终端输出判断agent状态，isPlainShell判断是否纯shell，markBusy标记忙碌。

Term_AgentNotify: 分析 fanbox Agent完成通知，正文带最后回复摘录(从终端缓冲区剥掉TUI框线/页脚捞正文)，标题带项目名，点通知拉回前台并切到对应标签(fanboxWin.focus)。

Term_AgentFalseComplete: 分析 fanbox Agent误完成守卫，agent阶段性收工但底部还挂后台任务(1 shell, 1 monitor still running)时不弹通知，等真正收工再报。

Term_ThemeSync: 分析 fanbox 终端配色与app皮肤联动，retheme方法动态切换xterm主题，JetBrains Mono/SF Mono等宽字+舒适行距+光标与选区配色随皮肤。

Term_Maximize: 分析 fanbox 终端铺满模式，双击顶栏空白等于终端挤掉文件区和预览铺满窗口，主动导航退出该状态让文件区回来。

Term_SendContext: 分析 fanbox 终端向agent发送上下文信息(sendContext)，写入cwd路径等环境信息辅助agent定位。

Term_LidGuard: 分析 fanbox 合盖保持运行，setLidIntent调pmset -b disablesleep 1，installSudoers配置免密码sudoers，refreshLidGuard刷新状态。

Term_ChineseWidth: 分析 fanbox 终端中文宽度修复，Unicode11 addon处理CJK双宽字符，全角标点进路径边界字符集。

Term_ScrollbackScan: 分析 fanbox 终端scrollback回扫路径识别(scanScrollbackFor)，从终端缓冲区提取可点击路径，缓存验证结果。

Term_WechatClawBot: 分析 fanbox 微信ClawBot集成，顶栏入口到连接弹窗状态机到OpenClaw网关到扫码登录到微信驱动本机agent，IPC通道wechat:env/login/disconnect/cancel/sessions/transcript。

Term_WechatQR: 分析 fanbox 微信二维码渲染，捕获openclaw channels login的stdout，正则提取liteapp.weixin.qq.com URL，用JS QR库(qrcode npm)重渲染清晰二维码，降级方案等宽字体渲染字符画。

Term_WechatTranscript: 分析 fanbox 微信对话内容读取，openclaw sessions list筛weixin channel到读~/.openclaw/agents/main/sessions/sid.jsonl到解析JSONL过滤user+assistant可读文本跳过system/tool噪声到渲染对话气泡。

Term_WechatAgentRegistry: 分析 fanbox 可连接agent注册表(CONNECTABLE_AGENTS)，静态配置Claude Code/Codex/Kimi/Hermes等，新增agent只需注册表加一行+OpenClaw配置加一段runtime，UI/IPC/弹窗逻辑不动。

Term_WechatStateMachine: 分析 fanbox 微信连接弹窗状态机：未装OpenClaw(状态A引导安装)到装了网关没起(状态B一键启动)到网关在线(状态C选agent)到生成二维码(状态D)到展示二维码(状态E)到扫码成功(状态F已连接)，二维码过期自动刷新。

Term_WechatGatewayMgmt: 分析 fanbox OpenClaw网关管理，检测安装状态(ocEnv)+拉起网关(ocRun)+登录后必须stop再start(不用restart因launchctl报I/O)+token存本地+手机登出即断连。

---

## Skills（技能系统）

Skill_MultiSourceScan: 分析 fanbox Skills透视的五源聚合扫描：~/.claude/skills+~/.codex/skills+~/.agents/skills+Claude插件(installed_plugins.json到installPath/skills)+项目级.claude/skills，scanSkillRoot递归+软链跟随。

Skill_FrontmatterParse: 分析 fanbox SKILL.md frontmatter解析(skillFrontmatter)，正则提取description字段，处理块标量(>-)和引号包裹，超240字符截断显示。

Skill_DescBudget: 分析 fanbox Skills Context预算条，全局常驻description总量vs SKILL_BUDGET_CHARS预算估算线，超限红色警示(超出部分被模型静默丢弃)，项目级skill不计入常驻预算。

Skill_HealthCheck: 分析 fanbox Skills健康检查：description超1536字符截断线(后段触发词模型看不见)标黄、缺frontmatter标红、缺SKILL.md标红、zip等残留物标灰，红黄绿标注+仅看问题过滤。

Skill_TriggerStats: 分析 fanbox Skills触发统计，解析Claude Code会话日志(jsonl里Skill tool_use模型自动触发+command-name用户手动调用)和Codex rollout(按会话去重)，45天触发次数+最后触发时间，一眼分出活跃和吃灰。

Skill_CrossSourceDup: 分析 fanbox 跨来源副本检测，同名skill出现在几处标N处副本(copies字段)，如同一skill在~/.claude/skills和~/.codex/skills都有，帮助用户清理冗余。

Skill_Toggle: 分析 fanbox Skills启停开关，停用等于移入_disabled/子目录(立即对模型不可见、不删文件、可逆)，不用官方skillOverrides(该配置在用户级有已知失效bug claude-code#50631)，软链接型skill先解析绝对目标再迁移避免相对链接断链。

Skill_TerminalInvoke: 分析 fanbox Skills终端调用，skill行拖进内嵌终端(或点详情按钮invokeSkillInTerm)，按会话里跑的agent自动注入/skill-name(Claude Code)或-name(Codex)。

Skill_Trash: 分析 fanbox Skills卸载，移到系统废纸篓随时可恢复(skillTrash调trashPath)，validateSkillDir路径校验只允许动最近一次扫描出来的skill目录杜绝任意路径删除。

Skill_Overview: 分析 fanbox Skills面板总览，总skill数+唯一skill数+活跃skill数(hits>0)+总触发次数+吃灰skill数(hits=0)+问题数，30秒缓存自动刷新。

Skill_ValidateDir: 分析 fanbox skill目录校验(validateSkillDir)，只允许操作最近一次扫描出来的skill目录，杜绝任意路径移动/删除，安全护栏。

---

## Agents Usage（智能体用量）

Agent_ClaudeTokenStats: 分析 fanbox Claude Code token统计(claudeUsage)，增量解析~/.claude/projects下jsonl会话日志(parseClaudeFile带offset增量+lastMsgId去重)，分近5h/今日/本周三档聚合，含input/output/cacheRead/cacheCreate分类统计。

Agent_ClaudeFileCache: 分析 fanbox Claude会话文件增量解析缓存(claudeFileCache Map)，offset追踪上次读到哪+lastMsgId去重同一消息多行落盘，文件被截断重写时自动重置，过期文件自动出缓存。

Agent_CodexUsage: 分析 fanbox Codex用量统计(codexUsage)，读~/.codex/sessions下rollout jsonl尾部抓最后一条带rate_limits的token_count(官方配额快照)，窗口重置后旧百分比归零标stale。

Agent_CodexStaleGuard: 分析 fanbox Codex配额快照过期守卫，capturedAt之后的窗口重置时间(resetsAt/resets_in_seconds/window_minutes)已过则usedPercent归零+stale标true，避免21小时前的57%失真数据误导。

Agent_ClaudeOAuthToken: 分析 fanbox Claude Code OAuth凭证获取(claudeOAuthToken)，macOS从Keychain读(security find-generic-password)，其他平台读~/.claude/.credentials.json，验expiresAt过期。

Agent_ClaudeOfficialLimits: 分析 fanbox Claude Code官方限额查询(claudeOfficialLimits)，用OAuth token调api.anthropic.com/api/oauth/usage，返回和/usage同源的5h窗口+7天配额百分比与重置时间，这是fanbox唯一出网请求。

Agent_CurlSysProxy: 分析 fanbox 系统代理自动检测(curlSysProxyLine)，打包App从Finder/Dock启动没有shell代理变量，curl直连被403地域拦截，此时读macOS系统代理(scutil --proxy)兜底，支持HTTPS/HTTP/SOCKS5。

Agent_CurlTlsFingerprint: 分析 fanbox 调用官方限额接口走系统curl而非Node https的原因：该接口按TLS指纹拦——同样请求头curl能200、Node直接403，token经stdin的curl配置传入不暴露在进程列表里。

Agent_UsagePanel: 分析 fanbox Agent用量面板，侧栏底部入口，展示Claude Code(近5h/今日/本周token+官方限额进度条)与Codex(5h窗口+周配额百分比+重置时间)，可开合，开着时每分钟自动刷新，30秒缓存。

Agent_UsageAutoRefresh: 分析 fanbox Agent用量面板自动刷新，展开时ensureStatusTick定时轮询，收起时停止，避免不必要网络请求。

Agent_MultiModelView: 分析 fanbox 多模型用量同屏对比，Claude Code和Codex用量数据并列展示，各带独立进度条和统计，未来可扩展Kimi/Hermes等模型。

Agent_RefreshHint: 分析 fanbox Agent用量刷新状态提示，数据显示最后更新时间(ago方法)，官方限额拿不到时自动回退本地统计，给用户明确的数据来源指示。

---

## 审计补充：遗漏的原子子功能

> 以下功能在首轮拆解中被遗漏，经逐行对照 server.js / electron/main.js / CHANGELOG 补出。

---

### File Manager 补充

File_DblClickFullscreen: 分析 fanbox 双击文本/代码文件直接全屏预览(单击仍是分栏轻预览)，双击md/html路径从终端点开也直接全屏，最贴合点一下就想看清这文件长啥样的意图。

File_PreviewAsEdit: 分析 fanbox 预览即编辑设计，代码/纯文本一进预览区就是可编辑态(和md一致)，不用再点编辑按钮，html(看的是渲染形态)/csv/tsv(表格视图)保持只读展示，降低操作步数。

File_TempFileFilter: 分析 fanbox 临时文件过滤策略，agent原子写临时文件(foo.swift.tmp.pid.hex这类.tmp在中段的命名)会触发fs.watch事件导致ENOENT和怪状态，过滤从.tmp结尾放宽到.tmp在结尾或后面还跟东西，从源头拦掉。

File_SaveImage: 分析 fanbox 图片编辑保存(saveImage)，支持dataUrl写入+newName另存为，覆盖原图加确认弹窗(不可逆+有损警告)，保存失败冒泡错误。

File_SidebarDragWindow: 分析 fanbox 按住侧栏空白可拖动窗口(对应issue #6)，侧栏整体不设为窗口拖拽区(否则收不到滚轮事件导致滚动时灵时卡)，拖窗用品牌区和顶栏。

File_ScrollbarStyle: 分析 fanbox 细圆角胶囊样式滚动条，带透明边距hover色跟随主题，不再是写死的深色粗条，提升视觉精致度。

File_GridTypeTint: 分析 fanbox 网格图标类型色磨砂圆角底座，类型色tint从13%加到20%提升辨识度，图标底座有圆角+半透明底色。

File_PortConflict: 分析 fanbox 端口占用检测，server.js启动时EADDRINUSE错误给出中文提示(FanBox很可能已经在运行了+直接打开浏览器访问就行+想另开一个换端口FANBOX_PORT=8080)。

File_EnvPort: 分析 fanbox 环境变量FANBOX_PORT自定义端口+FANBOX_NO_OPEN不自动开浏览器，支持开发/多实例场景。

File_DefaultRoots: 分析 fanbox 侧栏快速入口自动检测(defaultRoots)，候选主目录/桌面/文档/下载/Code/Projects/Developer，只显示实际存在的目录，跨平台适配。

---

### Terminal 补充

Term_QuitGuard: 分析 fanbox Cmd+Q退出守卫，还有终端在跑时(agent任务)退出前弹确认对话框(还有N个终端会话在运行/退出会终止正在运行的agent任务/确定退出？)，手滑防全灭；quitConfirmed标志防二次弹窗。

Term_ExitCleanup: 分析 fanbox 退出兜底清理，window-all-closed时kill所有PTY+关闭所有recorder流刷盘+恢复系统休眠(trySetDisableSleep false)；will-quit钩子确保无论怎么退(崩溃前正常退出)都恢复休眠绝不留禁休眠的烂摊子。

Term_ProcDetect: 分析 fanbox 终端前台进程名检测(pty:proc)，node-pty维护的process属性返回当前前台进程名，用于判断是裸shell(zsh/bash/sh/fish/login)还是正跑着claude/codex等程序，驱动Agent状态显示+启动按钮行为。

Term_OutputTail: 分析 fanbox 终端输出尾巴保留(termTails Map)，每个终端保留最后约4KB去ANSI后的输出文本，给微信agent跨终端感知(手机上看电脑在跑啥/卡哪)和完成通知摘录用。

Term_NativeEditMenu: 分析 fanbox 原生Edit菜单(buildMenu)，包含撤销/重做/剪切/复制/粘贴/全选，关键是Cmd+C/Cmd+V在终端里生效(Electron默认不拦截需role:editMenu)。

Term_ExternalLink: 分析 fanbox 外部链接走系统浏览器不在app里开新窗口(setWindowOpenHandler)，https链接shell.openExternal打开，避免在Electron窗口内导航走。

Term_RecordingPrune: 分析 fanbox 录像自动裁剪(recPrune)，保留最近60个文件+总量800MB上限，超了从最旧删起(正在录的跳过)，启动时和每次新录制触发裁剪，防磁盘无限涨。

Term_RecordingMeta: 分析 fanbox 录像cast文件私有元信息(fanbox字段)，记录cwd/cols/rows/startedAt/theme，回放/列表用这些元信息，asciinema标准解析器会忽略未知字段，空终端(小于700字节)不进列表防噪音。

Term_LsofDecode: 分析 fanbox 终端cwd获取的lsof中文路径解码(decodeLsofPath)，GUI启动的app没有UTF-8 locale时lsof把中文按字节转义成\xNN字面量，调lsof时显式给LC_ALL=en_US.UTF-8+这里留一层\xNN解码兜底。

Term_MultiDirWatch: 分析 fanbox 多目录监听增量diff(fs:watch-set)，浏览目录+每个终端会话所在的项目目录同时监听，前端发来期望监听集后做增量diff(关掉多余+补上新增)，一下午开多个项目跑agent时不在线的项目也能感知变更。

Term_LinuxWatchDegrade: 分析 fanbox Linux下fs.watch降级为非递归监听( macOS/Windows用FSEvents原生递归)，Linux递归不可靠故只监听当前目录，保证跨平台不崩。

Term_WechatILink: 分析 fanbox 微信ClawBot v2架构(非OpenClaw)，走腾讯iLink协议直连+本机claude/codex无头实例(electron/wechat/bridge.js含ilink.js+driver.js)，绕开OpenClaw中间层，延迟更低+不依赖第三方网关。

Term_WechatCrossTerm: 分析 fanbox 微信跨终端感知与控制(termControl)，微信agent可list所有终端(id/cwd/进程名/忙碌状态/最近输出尾巴)+向指定终端send文本(遥控)，手机上看电脑在跑啥并能远程操控。

Term_WechatConvMgmt: 分析 fanbox 微信对话管理IPC，wechat:conversation读对话+wechat:newConversation开新对话+wechat:compact压缩上下文+wechat:check主动探活+wechat:send发送消息，完整对话生命周期。

Term_WechatPersona: 分析 fanbox 微信人格/目标/cwd设置(wechat:setPersona/setTarget/setCwd)，可切换agent人格+指定目标agent+指定工作目录，多项目多agent灵活切换。

Term_WechatStayAwake: 分析 fanbox 微信离开不待机开关(wechat:setStayAwake)，独立于合盖继续运行的lidIntent，只要微信ClawBot连着就禁休眠(合盖/息屏也能远程操控)，断开微信自动恢复，首次需管理员密码装免密规则。

Term_AgentBinDetect: 分析 fanbox agent二进制检测(findAgentBin+codexOrganizeFlags)，在PATH中查找claude/codex可执行文件+检测codex的organize子命令支持，决定AI整理和启动按钮的可用性。

Term_ResizeEvent: 分析 fanbox 终端resize事件(pty:resize)，cols/rows变更时通知node-pty重排+记录resize事件到cast录像，TUI程序(vim/htop/claude code)重排正确不花屏。

Term_TermVerify: 分析 fanbox 终端验证接口(/api/term-verify)，验证指定目录能否打开终端(权限+存在性)，前端据此决定终端按钮状态。

