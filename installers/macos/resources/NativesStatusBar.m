// NativesStatusBar.m: macOS 顶部系统菜单栏（NSStatusItem）常驻“标签栏”实现

#import "NativesStatusBar.h"
#import "TokenMonitorHeaderView.h"
#import "NativesStatusBarPanel.h"
#import "NativesStatusBarData.h"
#import <WebKit/WebKit.h>
#import <libproc.h>
#import <signal.h>

// ===== 项目进程清场（防僵尸兜底） =====
// 场景：npm run statusbar 反复重启会残留旧实例（多托盘图标/过期面板）。
// 仅启动时清掉同二进制旧实例再拉起；退出只回收自己拥有的资源。
// 匹配口径 = 可执行文件路径完全一致
// （重建换装后路径不变，旧 inode 进程同样命中），不误伤其它项目组件。
void NativesStatusBarSweepProjectProcesses(void) {
    static NSString *selfPath = nil;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{
        char pathBuf[PROC_PIDPATHINFO_MAXSIZE];
        if (proc_pidpath(getpid(), pathBuf, sizeof(pathBuf)) > 0) {
            selfPath = [[NSString alloc] initWithUTF8String:pathBuf];
        }
    });
    if (selfPath.length == 0) return;

    int pidBuffer[8192];
    int pidCount = proc_listallpids(pidBuffer, (int)sizeof(pidBuffer));
    if (pidCount <= 0) return;

    NSMutableArray<NSNumber *> *victims = [NSMutableArray array];
    for (int i = 0; i < pidCount; i++) {
        pid_t pid = pidBuffer[i];
        if (pid <= 0 || pid == getpid()) continue;
        char pathBuf[PROC_PIDPATHINFO_MAXSIZE];
        if (proc_pidpath(pid, pathBuf, sizeof(pathBuf)) <= 0) continue; // 已退出或无权限
        NSString *path = [[NSString alloc] initWithUTF8String:pathBuf];
        if ([path isEqualToString:selfPath]) {
            [victims addObject:@(pid)];
        }
    }
    if (victims.count == 0) return;

    for (NSNumber *pidNum in victims) {
        kill((pid_t)pidNum.intValue, SIGTERM);
    }
    // 宽限 1 秒，仍存活者强杀
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:1.0];
    while (victims.count > 0 && [deadline timeIntervalSinceNow] > 0.0) {
        NSMutableArray<NSNumber *> *alive = [NSMutableArray array];
        for (NSNumber *pidNum in victims) {
            if (kill((pid_t)pidNum.intValue, 0) == 0) [alive addObject:pidNum];
        }
        if (alive.count == 0) return;
        victims = alive;
        [NSThread sleepForTimeInterval:0.05];
    }
    for (NSNumber *pidNum in victims) {
        char pathBuf[PROC_PIDPATHINFO_MAXSIZE];
        if (proc_pidpath(pidNum.intValue, pathBuf, sizeof(pathBuf)) > 0 &&
            [[NSString stringWithUTF8String:pathBuf] isEqualToString:selfPath]) kill(pidNum.intValue, SIGKILL);
    }
}

@interface PopoverPanel : NSPanel
@end

@implementation PopoverPanel
- (BOOL)canBecomeKeyWindow {
    return YES;
}
@end

@interface NativesStatusBar () <NSMenuDelegate, WKScriptMessageHandler, WKNavigationDelegate>
@property (nonatomic, strong) NSMenu *statusMenu;
@property (nonatomic, strong) NSPanel *popoverPanel;
@property (nonatomic, strong) WKWebView *webView;
@property (nonatomic, copy) NSString *latestDisplayText;
@property (nonatomic, copy) NSString *latestTokensText;
@property (nonatomic, copy) NSString *latestCostText;
@property (nonatomic, copy) NSString *worstLimitText;
@property (nonatomic, copy) NSString *latestTooltip;
@property (nonatomic, assign) double worstLimitPercent; // 0~100，无数据为 -1
@property (nonatomic, strong) NSTimer *refreshTimer;
// 最近一次 8766 查询的账号额度快照（ADR-0033 Phase 2；8766 不可达时来自缓存兜底）
@property (nonatomic, copy) NSArray *lastQuotaAccounts;
// 额度数据来源标注："8766" / "cache" / "none"（none 时面板显示"模型服务未运行"）
@property (nonatomic, copy) NSString *lastQuotaSource;
// 顶栏双行轮播：帧序列（用量帧 + 各渠道额度帧）、1 秒渲染 / 20 秒切换
@property (nonatomic, copy) NSArray *carouselFrames;
@property (nonatomic, assign) NSUInteger carouselTick;
@property (nonatomic, strong) NSTimer *carouselTimer;
// 8766 不可达时按需拉起的 model-host worker（生命周期跟随顶栏进程，非常驻）
@property (nonatomic, strong) NSTask *modelHostTask;
// 持有 worker stdin 的写端：顶栏退出 → 管道 EOF → worker 退出（ADR-0020 非常驻边界）
@property (nonatomic, strong) NSPipe *modelHostStdinPipe;
@property (nonatomic, strong) NSLock *modelHostSpawnLock;
@property (nonatomic, assign) BOOL usageFetchInFlight;
@property (nonatomic, assign) BOOL panelFetchInFlight;
@property (nonatomic, assign) BOOL pendingPanelFetch;
@property (nonatomic, assign) BOOL pendingPanelRefresh;
@property (nonatomic, assign) NSUInteger panelGeneration;
@property (nonatomic, assign) NSUInteger usageGeneration;
@property (atomic, assign) BOOL shuttingDown;
@end

@implementation NativesStatusBar

+ (instancetype)sharedBar {
    static NativesStatusBar *instance = nil;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{
        instance = [[NativesStatusBar alloc] init];
    });
    return instance;
}

- (instancetype)init {
    self = [super init];
    if (self) {
        _displayMode = NativesStatusBarDisplayModeBoth;
        _runtimeBaseUrl = @"http://127.0.0.1:8766"; // ADR-0033 / ADR-0035: AI 查询统一走 model-host 8766
        _latestDisplayText = @"--";
        _latestTokensText = @"--";
        _latestCostText = @"--";
        _worstLimitPercent = -1;
        _latestTooltip = @"Token Usage";
        _modelHostSpawnLock = [NSLock new];
    }
    return self;
}

- (void)setupStatusItem {
    if (self.statusItem) return;

    self.statusItem = [[NSStatusBar systemStatusBar] statusItemWithLength:NSVariableStatusItemLength];
    NSStatusBarButton *button = self.statusItem.button;

    if (button) {
        button.image = [self createTemplateIcon];
        button.imagePosition = NSImageLeft;
        button.title = @" --";
        button.toolTip = self.latestTooltip;
        button.target = self;
        button.action = @selector(togglePanel);
    }

    [self buildMenu];

    // 定时轮询状态（每 30 秒更新一次）
    self.refreshTimer = [NSTimer scheduledTimerWithTimeInterval:30.0
                                                         target:self
                                                       selector:@selector(fetchAndUpdateState)
                                                       userInfo:nil
                                                        repeats:YES];
    [self fetchAndUpdateState];

    // 顶栏双行轮播：每秒渲染（额度倒计时走秒），每 20 秒切换一帧
    self.carouselTimer = [NSTimer timerWithTimeInterval:1.0
                                                 target:self
                                               selector:@selector(carouselTickHandler)
                                               userInfo:nil
                                                repeats:YES];
    [[NSRunLoop mainRunLoop] addTimer:self.carouselTimer forMode:NSRunLoopCommonModes];
    [self rebuildQuotaFrames];

    // 启动只读已有额度；上游探测只由显式刷新触发。
    [self fetchLimitsWithCompletion:^(NSArray *items) {
        [self rebuildQuotaFrames];
    } live:NO];

    // 启动预热：Chrome 断开 NM 后 model-host 已退出，后台按需拉起数据源
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        [self ensureModelHostAlive];
    });
}

- (NSImage *)createTemplateIcon {
    // 绘制 18x18 矢量闪电图元（自适应深色/浅色菜单栏）
    NSImage *img = [NSImage imageWithSize:NSMakeSize(18, 18) flipped:NO drawingHandler:^BOOL(NSRect dstRect) {
        NSBezierPath *path = [NSBezierPath bezierPath];
        [path moveToPoint:NSMakePoint(10.0, 17.0)];
        [path lineToPoint:NSMakePoint(4.0, 9.0)];
        [path lineToPoint:NSMakePoint(9.0, 9.0)];
        [path lineToPoint:NSMakePoint(8.0, 1.0)];
        [path lineToPoint:NSMakePoint(14.0, 9.0)];
        [path lineToPoint:NSMakePoint(9.5, 9.0)];
        [path closePath];

        [[NSColor blackColor] setFill];
        [path fill];
        return YES;
    }];

    [img setTemplate:YES];
    return img;
}

#pragma mark - 弹窗面板生命周期

- (NSPanel *)ensurePanel {
    if (self.popoverPanel) return self.popoverPanel;

    CGFloat width = 380.0, height = 620.0;
    self.popoverPanel = [[PopoverPanel alloc] initWithContentRect:NSMakeRect(0, 0, width, height)
                                                   styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    self.popoverPanel.opaque = NO;
    self.popoverPanel.backgroundColor = [NSColor clearColor];
    self.popoverPanel.hasShadow = YES;
    self.popoverPanel.level = NSStatusWindowLevel;
    self.popoverPanel.hidesOnDeactivate = NO;
    self.popoverPanel.releasedWhenClosed = NO;
    self.popoverPanel.collectionBehavior = NSWindowCollectionBehaviorFullScreenAuxiliary;

    // 苹果原生毛玻璃底层视图（BehindWindow 深度实时磨砂渲染，透出桌面与背景窗口；
    // 固定深色外观，浅色系统下依然保持深色玻璃与浅色文字的可读性）
    NSVisualEffectView *vibrantGlass = [[NSVisualEffectView alloc] initWithFrame:NSMakeRect(0, 0, width, height)];
    vibrantGlass.material = NSVisualEffectMaterialUnderWindowBackground; // 最透：直接采样桌面与背景窗口
    vibrantGlass.blendingMode = NSVisualEffectBlendingModeBehindWindow;
    vibrantGlass.state = NSVisualEffectStateActive;
    vibrantGlass.appearance = [NSAppearance appearanceNamed:NSAppearanceNameVibrantDark];
    vibrantGlass.wantsLayer = YES;
    vibrantGlass.layer.cornerRadius = 16.0;
    vibrantGlass.layer.masksToBounds = YES;
    vibrantGlass.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;

    self.webView = [[WKWebView alloc] initWithFrame:vibrantGlass.bounds
                                      configuration:[self webViewConfig]];
    self.webView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    self.webView.navigationDelegate = self;
    [self.webView setValue:@NO forKey:@"drawsBackground"];
    if (@available(macOS 12.0, *)) {
        self.webView.underPageBackgroundColor = [NSColor clearColor];
    }

    [vibrantGlass addSubview:self.webView];
    self.popoverPanel.contentView = vibrantGlass;

    NSNotificationCenter *nc = [NSNotificationCenter defaultCenter];
    [nc addObserver:self selector:@selector(panelClosed) name:NSWindowWillCloseNotification object:self.popoverPanel];
    return self.popoverPanel;
}

- (void)closePanel {
    self.panelGeneration++;
    self.pendingPanelFetch = NO; self.pendingPanelRefresh = NO;
    if (self.clickOutsideMonitor) {
        [NSEvent removeMonitor:self.clickOutsideMonitor];
        self.clickOutsideMonitor = nil;
    }
    if (self.popoverPanel && self.popoverPanel.isVisible) {
        [self.popoverPanel orderOut:nil];
    }
}

- (WKWebViewConfiguration *)webViewConfig {
    WKWebViewConfiguration *config = [[WKWebViewConfiguration alloc] init];
    WKUserContentController *ucc = [[WKUserContentController alloc] init];
    [ucc addScriptMessageHandler:self name:@"native"];
    config.userContentController = ucc;
    return config;
}

- (void)togglePanel {
    if (self.popoverPanel && self.popoverPanel.isVisible) {
        [self closePanel];
        return;
    }

    NSPanel *panel = [self ensurePanel];
    self.panelGeneration++;

    NSRect iconRect = [self.statusItem.button frame];
    NSWindow *buttonWindow = [self.statusItem.button window];
    if (buttonWindow) {
        iconRect = [self.statusItem.button convertRect:self.statusItem.button.bounds toView:nil];
        iconRect = [buttonWindow convertRectToScreen:iconRect];
    }
    NSScreen *screen = NSScreen.mainScreen;
    NSRect visible = screen.visibleFrame;
    CGFloat width = 380.0, height = 620.0;
    CGFloat x = iconRect.origin.x + iconRect.size.width / 2.0 - width / 2.0;
    x = MAX(visible.origin.x + 4, MIN(x, visible.origin.x + visible.size.width - width - 4));
    CGFloat y = iconRect.origin.y - height - 8;
    y = MAX(visible.origin.y + 4, y);
    [panel setFrame:NSMakeRect(x, y, width, height) display:YES];

    [self.webView loadHTMLString:NativesStatusBarPanelHTML() baseURL:nil];
    [panel makeKeyAndOrderFront:nil];
    [panel makeFirstResponder:self.webView];

    // 全局鼠标监听：仅在点击面板外部（且不在状态栏按钮上）时才关闭面板，
    // 面板内部的所有点击（Tab、滚动、刷新）绝不会误关闭窗口。
    if (!self.clickOutsideMonitor) {
        __weak typeof(self) weakSelf = self;
        self.clickOutsideMonitor = [NSEvent addGlobalMonitorForEventsMatchingMask:(NSEventMaskLeftMouseDown | NSEventMaskRightMouseDown) handler:^(NSEvent *event) {
            dispatch_async(dispatch_get_main_queue(), ^{
                if (!weakSelf || !weakSelf.popoverPanel || !weakSelf.popoverPanel.isVisible) return;
                NSPoint mouseLoc = [NSEvent mouseLocation];
                NSRect panelFrame = weakSelf.popoverPanel.frame;
                NSRect btnFrame = NSZeroRect;
                if (weakSelf.statusItem.button.window) {
                    btnFrame = [weakSelf.statusItem.button.window convertRectToScreen:weakSelf.statusItem.button.frame];
                }
                if (!NSPointInRect(mouseLoc, panelFrame) && !NSPointInRect(mouseLoc, btnFrame)) {
                    [weakSelf closePanel];
                }
            });
        }];
    }
}

- (void)webView:(WKWebView *)webView didFinishNavigation:(WKNavigation *)navigation {
    [self fetchPanelData];
}

- (void)panelClosed {
    self.panelGeneration++;
    [NSNotificationCenter.defaultCenter removeObserver:self name:NSWindowWillCloseNotification object:self.popoverPanel];
    [self.webView.configuration.userContentController removeScriptMessageHandlerForName:@"native"];
    if (self.clickOutsideMonitor) {
        [NSEvent removeMonitor:self.clickOutsideMonitor];
        self.clickOutsideMonitor = nil;
    }
    self.popoverPanel = nil;
    self.webView = nil;
}

- (void)fetchPanelData {
    // 面板打开：只读本地真实数据，不发起额度上游请求（避免频繁请求触发账号风控）
    [self fetchPanelDataLive:NO completion:nil];
}

// 并行拉取面板状态（使用情况/总量/会话/趋势）与账号额度，两路都落地后回调 completion（主线程）；
// live 仅在用户手动点「刷新」时为 YES，此时才实时请求 Codex 额度上游
- (void)fetchPanelDataLive:(BOOL)live completion:(void (^)(void))completion {
    if (!self.webView) {
        if (completion) completion();
        return;
    }
    if (self.panelFetchInFlight) {
        self.pendingPanelFetch = YES;
        self.pendingPanelRefresh |= live;
        return;
    }
    self.panelFetchInFlight = YES;
    NSUInteger generation = self.panelGeneration;
    NSUInteger usageGeneration = ++self.usageGeneration;

    dispatch_group_t group = dispatch_group_create();

    dispatch_group_enter(group);
    // 面板状态只走 Model Host 8766 的稳定 Usage Query 合约。
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSDictionary *state = [self readPanelState];
        dispatch_async(dispatch_get_main_queue(), ^{
            if (self.webView && generation == self.panelGeneration && !self.shuttingDown) {
                if (state) {
                    NSData *stateData = [NSJSONSerialization dataWithJSONObject:state options:0 error:nil];
                    if (stateData && usageGeneration == self.usageGeneration) {
                        [self updateWithStateJson:[[NSString alloc] initWithData:stateData encoding:NSUTF8StringEncoding]];
                    }
	                    id panel = state[@"panel"];
	                    if (panel) {
	                        NSArray *currentLimits = [self currentLimitsSnapshot];
	                        NSMutableDictionary *msg = [@{@"type": @"panel", @"panel": panel} mutableCopy];
	                        if (currentLimits.count > 0) {
	                            msg[@"limits"] = currentLimits;
	                        }
	                        msg[@"quotaSource"] = self.lastQuotaSource ?: @"none";
	                        msg[@"usageSource"] = state[@"usageSource"] ?: @"8766";
	                        msg[@"usageOffline"] = @NO;
	                        [self postToPanel:msg];
	                    }
	                } else {
	                    // 8766 不可达：显式空态 + 离线标注，严禁把「取不到」渲染成真实的 0
	                    NSDictionary *emptyPanel = @{
                        @"today": @{@"totalTokens": @0, @"costUsd": @0.0, @"priced": @NO},
	                        @"thisWeek": @{@"totalTokens": @0, @"costUsd": @0.0},
	                        @"thisMonth": @{@"totalTokens": @0, @"costUsd": @0.0},
	                        @"allTime": @{@"totalTokens": @0, @"costUsd": @0.0},
	                        @"tools": @[],
	                        @"toolsByPeriod": @{@"today": @[], @"thisWeek": @[], @"thisMonth": @[], @"allTime": @[]},
	                        @"sessions": @[],
	                        @"trends": @[]
	                    };
	                    [self postToPanel:@{@"type": @"panel", @"panel": emptyPanel,
                                        @"quotaSource": self.lastQuotaSource ?: @"none",
                                        @"usageOffline": @YES}];
	                }
            }
            dispatch_group_leave(group);
        });
    });

    dispatch_group_enter(group);
    [self fetchLimitsWithCompletion:^(NSArray *items) {
        if (self.webView && generation == self.panelGeneration && !self.shuttingDown) {
            [self postToPanel:@{@"type": @"limits", @"items": items ?: @[],
                                @"quotaSource": self.lastQuotaSource ?: @"none"}];
        }
        dispatch_group_leave(group);
    } live:live];

    dispatch_group_notify(group, dispatch_get_main_queue(), ^{
        self.panelFetchInFlight = NO;
        if (completion && generation == self.panelGeneration && !self.shuttingDown) completion();
        // 额度数据（Codex 实时结果 / 扩展写入的缓存）落地后重建顶栏轮播帧
        [self rebuildQuotaFrames];
        if (self.pendingPanelFetch && self.popoverPanel.isVisible && !self.shuttingDown) {
            BOOL refresh = self.pendingPanelRefresh;
            self.pendingPanelFetch = NO; self.pendingPanelRefresh = NO;
            if (refresh) [self performPanelRefresh]; else [self fetchPanelData];
        }
    });
}

- (NSArray *)currentLimitsSnapshot {
    return self.lastQuotaAccounts ?: @[];
}

// 账号额度解析（后台线程，ADR-0033 Phase 2 / 架构 Wave 2 终态）：
//   额度唯一经 8766 只读查询面获取——
//   live=YES（用户手动刷新）时先触发 POST /ai/quota/refresh（model-host 内部
//   执行上游探测，顶栏不再直连任何上游端点/Keychain），随后 GET /ai/quota；
//   live=NO（面板打开/30s 轮询）只读最近结果，不触发上游请求（反封号口径不变）。
//   8766 不可达时回落 quota-cache.json 只读兜底；两者皆空即"模型服务未运行"空态。
//   彻底废除对本地 state.json、~/.codex/auth.json 以及 limits_cache 权威表的违规直读。
- (void)fetchLimitsWithCompletion:(void (^)(NSArray *items))completion live:(BOOL)live {
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        [self ensureModelHostAlive]; // 额度查询同样依赖 8766；不可达时先按需拉起
        NSString *quotaSource = @"none";
        NSArray *quotaAccounts = FetchQuotaAccountsViaAIQuery(live, &quotaSource);
        dispatch_async(dispatch_get_main_queue(), ^{
            self.lastQuotaAccounts = quotaAccounts;
            self.lastQuotaSource = quotaSource;
            completion(quotaAccounts ?: @[]);
        });
    });
}

// 刷新动作（仅由用户点击触发）：重拉面板状态，并经 8766 触发一次上游额度刷新，
// 全部落地后通知面板停止转圈。上游请求只由 model-host 在本动作内发起；
// 面板打开与 30s 状态轮询均不触发。运行时不可达时显示明确空态。
- (void)performPanelRefresh {
    if (!self.webView) return;
    [self fetchPanelDataLive:YES completion:^{
        [self postToPanel:@{@"type": @"refreshDone"}];
    }];
}

#pragma mark - model-host 按需拉起（ADR-0033 数据源保活）

// 确保 model-host（8766 数据权威）可用。model-host 由 Chrome NM 连接按需拉起、
// 客户端断开即退出（resident=false），因此顶栏场景下 8766 长期不可达，实时
// 用量/额度/刷新全部失效。此处复用其既有 worker 模式按需重建数据源：
//   - 首实例 Acquire 成功 → 成为 worker，8766 上线；已有首实例 → 本进程退化为
//     bridge，同样不冲突（singleinstance socket 语义）。
//   - 生命周期严格跟随顶栏：stdin 写端由顶栏持有，顶栏退出 → EOF → worker 退出；
//     用户显式开启常驻设置时按其选择继续驻留。不违反 ADR-0020 非常驻边界。
- (BOOL)ensureModelHostAlive {
    [self.modelHostSpawnLock lock];
    @try {
        if (self.shuttingDown) return NO;
        if (AIQueryHealthzAlive(1.0)) return YES;
        NSTask *existing = self.modelHostTask;
        if (existing && existing.isRunning) {
            // 已按需拉起过且仍在运行（可能尚在 Engine.Restore 启动期）：只等就绪，
            // 绝不重复 spawn——Acquire 失败的实例会退化为常挂的 bridge 进程。
            // 就绪等待必须真实让渡 CPU：healthz 对未监听端口是毫秒级拒绝，
            // 无 sleep 会瞬间烧完尝试次数提前返回 NO（生命周期整改 T4）。
            for (int attempt = 0; attempt < 60; attempt++) {
                if (self.shuttingDown) return NO;
                if (AIQueryHealthzAlive(0.5)) return YES;
                if (!existing.isRunning) break;
                [NSThread sleepForTimeInterval:0.1];
            }
            return NO;
        }
        NSString *executable = LocateModelHostExecutable();
        if (!executable) return NO;

        NSTask *task = [[NSTask alloc] init];
        task.executableURL = [NSURL fileURLWithPath:executable];
        task.arguments = @[@"--model-host-worker"];
        NSPipe *stdinPipe = [NSPipe pipe];
        NSPipe *stdoutPipe = [NSPipe pipe];
        NSPipe *stderrPipe = [NSPipe pipe];
        task.standardInput = stdinPipe;
        task.standardOutput = stdoutPipe;
        task.standardError = stderrPipe;
        // worker 的 NM 输出与日志无消费方，持续 drain，防管道缓冲写满阻塞 worker
        stdoutPipe.fileHandleForReading.readabilityHandler = ^(NSFileHandle *handle) {
            [handle availableData];
        };
        stderrPipe.fileHandleForReading.readabilityHandler = ^(NSFileHandle *handle) {
            [handle availableData];
        };
        __weak typeof(self) weakSelf = self;
        task.terminationHandler = ^(NSTask *finished) {
            ((NSPipe *)finished.standardOutput).fileHandleForReading.readabilityHandler = nil;
            ((NSPipe *)finished.standardError).fileHandleForReading.readabilityHandler = nil;
            [weakSelf modelHostDidExit:finished];
        };
        NSError *launchError = nil;
        if (![task launchAndReturnError:&launchError]) return NO;
        self.modelHostTask = task;
        self.modelHostStdinPipe = stdinPipe;

        // 等待 8766 就绪（Engine.Restore + 端口绑定）；同样按 0.1s 步进让渡 CPU，
        // 使就绪窗口真实覆盖 Engine 启动耗时（生命周期整改 T4）。
        for (int attempt = 0; attempt < 50; attempt++) {
            if (self.shuttingDown) return NO;
            if (AIQueryHealthzAlive(0.5)) return YES;
            if (!task.isRunning) return NO;
            [NSThread sleepForTimeInterval:0.1];
        }
        return NO;
    } @finally {
        [self.modelHostSpawnLock unlock];
    }
}

- (void)modelHostDidExit:(NSTask *)finished {
    [self.modelHostSpawnLock lock];
    if (self.modelHostTask == finished) {
        self.modelHostTask = nil;
        self.modelHostStdinPipe = nil;
    }
    [self.modelHostSpawnLock unlock];
}

- (void)postToPanel:(NSDictionary *)dict {
    if (!self.webView || !dict) return;
    NSData *data = [NSJSONSerialization dataWithJSONObject:dict options:0 error:nil];
    if (!data) return;
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    [self.webView evaluateJavaScript:[NSString stringWithFormat:@"window.postMessage(%@, '*')", json] completionHandler:nil];
}

- (void)userContentController:(WKUserContentController *)userContentController
      didReceiveScriptMessage:(WKScriptMessage *)message {
    if (![message.body isKindOfClass:[NSDictionary class]]) return;
    NSString *action = message.body[@"action"];
    if (![action isKindOfClass:[NSString class]]) return;
    // 刷新动作：先回源刷新额度，再重拉面板数据（使用情况 + 额度与余量），不关闭弹窗、不打开应用
    if ([action isEqualToString:@"refresh"]) {
        [self performPanelRefresh];
        return;
    }
    [self openTargetApp:action];
    [self closePanel];
}

#pragma mark - NSMenuDelegate 与上下文菜单

- (void)menuDidClose:(NSMenu *)menu {
    (void)menu;
}

- (void)actTogglePanel:(id)sender {
    (void)sender;
    [self togglePanel];
}

- (void)buildMenu {
    self.statusMenu = [[NSMenu alloc] initWithTitle:@"Token Monitor"];
    self.statusMenu.delegate = self;

    TokenMonitorHeaderView *headerView = [[TokenMonitorHeaderView alloc]
        initWithFrame:NSMakeRect(0, 0, 240, self.worstLimitPercent >= 0 ? 78.0 : 60.0)
         displayText:self.latestDisplayText
          limitText:self.worstLimitText
       limitPercent:self.worstLimitPercent];
    NSMenuItem *headerItem = [[NSMenuItem alloc] initWithTitle:@"Token Monitor"
                                                        action:nil
                                                 keyEquivalent:@""];
    headerItem.view = headerView;
    [headerItem setEnabled:NO];
    [self.statusMenu addItem:headerItem];

    [self.statusMenu addItem:[NSMenuItem separatorItem]];

    NSMenuItem *openUsageItem = [[NSMenuItem alloc] initWithTitle:@"打开 Token Usage 仪表板"
                                                           action:@selector(actOpenTokenUsage)
                                                    keyEquivalent:@"u"];
    openUsageItem.target = self;
    [self.statusMenu addItem:openUsageItem];

    NSMenuItem *openSpaceItem = [[NSMenuItem alloc] initWithTitle:@"打开 Natives 个人空间"
                                                           action:@selector(actOpenSpace)
                                                    keyEquivalent:@"s"];
    openSpaceItem.target = self;
    [self.statusMenu addItem:openSpaceItem];

    NSMenuItem *openFilesItem = [[NSMenuItem alloc] initWithTitle:@"打开 Files 文件管理"
                                                           action:@selector(actOpenFiles)
                                                    keyEquivalent:@"f"];
    openFilesItem.target = self;
    [self.statusMenu addItem:openFilesItem];

    [self.statusMenu addItem:[NSMenuItem separatorItem]];

    NSMenuItem *refreshItem = [[NSMenuItem alloc] initWithTitle:@"立即刷新统计"
                                                         action:@selector(fetchAndUpdateState)
                                                  keyEquivalent:@"r"];
    refreshItem.target = self;
    [self.statusMenu addItem:refreshItem];

    NSMenuItem *displayModeItem = [[NSMenuItem alloc] initWithTitle:@"顶栏显示排版"
                                                             action:nil
                                                      keyEquivalent:@""];
    NSMenu *displaySubmenu = [[NSMenu alloc] initWithTitle:@"排版模式"];

    NSArray *modes = @[
        @{@"title": @"Token 与费用 (1.25M · $3.42)", @"mode": @(NativesStatusBarDisplayModeBoth)},
        @{@"title": @"仅 Token (1.25M)", @"mode": @(NativesStatusBarDisplayModeTokens)},
        @{@"title": @"仅费用 ($3.42)", @"mode": @(NativesStatusBarDisplayModeCost)},
        @{@"title": @"额度进度条/百分比", @"mode": @(NativesStatusBarDisplayModeBars)},
        @{@"title": @"仅图标", @"mode": @(NativesStatusBarDisplayModeIconOnly)}
    ];

    for (NSDictionary *m in modes) {
        NSInteger modeVal = [m[@"mode"] integerValue];
        NSMenuItem *subItem = [[NSMenuItem alloc] initWithTitle:m[@"title"]
                                                         action:@selector(actChangeDisplayMode:)
                                                  keyEquivalent:@""];
        subItem.target = self;
        subItem.tag = modeVal;
        subItem.state = (self.displayMode == modeVal) ? NSControlStateValueOn : NSControlStateValueOff;
        [displaySubmenu addItem:subItem];
    }
    displayModeItem.submenu = displaySubmenu;
    [self.statusMenu addItem:displayModeItem];

    [self.statusMenu addItem:[NSMenuItem separatorItem]];

    NSMenuItem *quitItem = [[NSMenuItem alloc] initWithTitle:@"退出"
                                                      action:@selector(actQuit)
                                               keyEquivalent:@"q"];
    quitItem.target = self;
    [self.statusMenu addItem:quitItem];
}

- (void)actChangeDisplayMode:(NSMenuItem *)sender {
    self.displayMode = sender.tag;
    [self refreshButtonUI];
    [self buildMenu];
}

- (void)refreshButtonUI {
    NSStatusBarButton *button = self.statusItem.button;
    if (!button) return;

    button.toolTip = self.latestTooltip;

    if (self.displayMode == NativesStatusBarDisplayModeBoth) {
        // 双行轮播模式（默认）：数字在前、符号在后，1 秒渲染 / 3 秒切换
        [self renderCarouselFrame];
        return;
    }

    button.imagePosition = NSImageLeft;
    button.attributedTitle = [[NSAttributedString alloc] initWithString:@""];
    NSString *title = @"";
    switch (self.displayMode) {
        case NativesStatusBarDisplayModeTokens:
            title = [NSString stringWithFormat:@" %@", self.latestTokensText];
            break;
        case NativesStatusBarDisplayModeCost:
            title = [NSString stringWithFormat:@" %@", self.latestCostText];
            break;
        case NativesStatusBarDisplayModeBars:
            title = self.worstLimitText.length > 0 ? [NSString stringWithFormat:@" [%@]", self.worstLimitText] : @" [--]";
            break;
        case NativesStatusBarDisplayModeIconOnly:
            title = @"";
            break;
        default:
            title = [NSString stringWithFormat:@" %@", self.latestDisplayText];
            break;
    }
    button.title = title;
}

#pragma mark - 顶栏双行轮播

// 倒计时文案：随剩余时长自动缩写（秒 → 分秒 → 时分 → 天）

// 倒计时紧凑文案：数字在前、单位在后，最多两个单位（1H20M / 6D12H / 45S）
- (NSString *)compactCountdownFromEpoch:(double)epoch {
    if (epoch <= 0) return nil;
    NSTimeInterval seconds = epoch - [NSDate date].timeIntervalSince1970;
    if (seconds <= 0) return @"已重置";
    long long sec = (long long)seconds;
    if (sec < 60) return [NSString stringWithFormat:@"%lldS", sec];
    if (sec < 3600) return [NSString stringWithFormat:@"%lldM%lldS", sec / 60, sec % 60];
    if (sec < 86400) return [NSString stringWithFormat:@"%lldH%lldM", sec / 3600, (sec % 3600) / 60];
    return [NSString stringWithFormat:@"%lldD%lldH", sec / 86400, (sec % 86400) / 3600];
}

// 窗口类型缩写：5小时额度→5H、周额度→周、日额度→日、月额度→月
- (NSString *)quotaWindowShortLabel:(NSString *)label {
    NSString *raw = label ?: @"";
    NSInteger priority = QuotaWindowSortPriority(raw);
    if (priority == 1) return @"5H";
    if (priority == 2) return @"日";
    if (priority == 3) return @"周";
    if (priority == 4) return @"月";
    return raw;
}

// 倒计时文案：随剩余时长自动缩写（秒 → 分秒 → 时分 → 天）
- (NSString *)countdownTextFromEpoch:(double)epoch {
    if (epoch <= 0) return nil;
    NSTimeInterval seconds = epoch - [NSDate date].timeIntervalSince1970;
    if (seconds <= 0) return @"已重置";
    long long sec = (long long)seconds;
    if (sec < 60) return [NSString stringWithFormat:@"%lld秒后重置", sec];
    if (sec < 3600) return [NSString stringWithFormat:@"%lld分%lld秒后重置", sec / 60, sec % 60];
    if (sec < 86400) return [NSString stringWithFormat:@"%lld时%lld分后重置", sec / 3600, (sec % 3600) / 60];
    return [NSString stringWithFormat:@"%lld天后重置", sec / 86400];
}

// 百分比：整数不带小数，非整数保留一位（数字在前、% 后置）
- (NSString *)percentText:(double)pct {
    return [NSString stringWithFormat:@"%.0f%%", pct];
}

// 费用：$ 前置改为数字前置、$ 后置（"3.42$"）
- (NSString *)costNumberFirst:(NSString *)cost {
    if ([cost hasPrefix:@"$"]) return [NSString stringWithFormat:@"%@$", [cost substringFromIndex:1]];
    return cost ?: @"--";
}

// 渠道短名（额度帧上行行首标注，标明这是谁的额度）

- (NSDictionary *)selectDisplayWindow:(NSArray *)windows {
    NSDictionary *best5h = nil, *bestWeekly = nil, *bestOther = nil;
    double pct5h = 101.0, pctWeekly = 101.0, pctOther = 101.0;
    for (NSDictionary *w in windows) {
        if (![w isKindOfClass:[NSDictionary class]]) continue;
        NSString *label = [w[@"label"] isKindOfClass:[NSString class]] ? w[@"label"] : @"";
        NSNumber *pctNum = [w[@"pct"] isKindOfClass:[NSNumber class]] ? w[@"pct"] : nil;
        double pct = pctNum ? pctNum.doubleValue : 101.0;
        if (QuotaWindowSortPriority(label) == 1) {
            if (pct < pct5h) { pct5h = pct; best5h = w; }
        } else if (QuotaWindowSortPriority(label) == 3) {
            if (pct < pctWeekly) { pctWeekly = pct; bestWeekly = w; }
        } else {
            if (pct < pctOther) { pctOther = pct; bestOther = w; }
        }
    }
    // "耗尽" = 剩余 ≤ 0（Codex 的限额触发同样表现为剩余 0）
    BOOL fiveHourExhausted = (best5h == nil || pct5h <= 0.0);
    BOOL weeklyExhausted = (bestWeekly == nil || pctWeekly <= 0.0);
    if (fiveHourExhausted && weeklyExhausted) return bestWeekly ?: bestOther ?: best5h; // ③ 等周重置
    return best5h ?: bestWeekly ?: bestOther;                                            // ①② 等5小时重置
}

// 从渠道窗口列表挑选上行（5小时额度）与下行（周额度）；
// 找不到标准窗口时回退到剩余最少 / 次少的两条

- (void)rebuildQuotaFrames {
        NSArray *items = [self currentLimitsSnapshot];

        NSMutableArray *quotaFrames = [NSMutableArray array];
        for (NSDictionary *item in items) {
            if (![item isKindOfClass:[NSDictionary class]]) continue;
            NSArray *windows = [item[@"windows"] isKindOfClass:[NSArray class]] ? item[@"windows"] : @[];
            if (windows.count == 0) continue; // 无额度数据的渠道直接跳过

            // 检查该渠道内部是否有不同模型分组（例如 Antigravity 同时具备 Claude/GPT 与 Gemini 独立额度）
            NSMutableDictionary<NSString *, NSMutableArray *> *groups = [NSMutableDictionary dictionary];
            for (NSDictionary *w in windows) {
                if (![w isKindOfClass:[NSDictionary class]]) continue;
                NSString *grp = [w[@"group"] isKindOfClass:[NSString class]] && [w[@"group"] length] > 0
                    ? w[@"group"] : ExtractModelGroupFromLabel(w[@"label"]);
                if (grp.length == 0) grp = item[@"providerId"] ?: @"";
                if (!groups[grp]) groups[grp] = [NSMutableArray array];
                [groups[grp] addObject:w];
            }

            for (NSString *grpName in groups) {
                // 谷歌(antigravity)渠道只轮询 Gemini 额度，Claude 等其它模型组跳过不参与轮播
                if ([item[@"providerId"] isEqualToString:@"antigravity"] && ![grpName containsString:@"Gemini"]) {
                    continue;
                }

                NSArray *grpWindows = groups[grpName];
                NSDictionary *selected = [self selectDisplayWindow:grpWindows];
                if (!selected) continue;

                double sortPct = 101.0;
                if ([selected[@"pct"] isKindOfClass:[NSNumber class]]) sortPct = [selected[@"pct"] doubleValue];

                NSString *subProvider = item[@"providerId"] ?: @"";
                NSString *displayName = @"";
                if ([grpName containsString:@"Claude"]) {
                    displayName = @"Claude";
                    subProvider = @"claude"; // 图标用 C
                } else if ([grpName containsString:@"Gemini"]) {
                    displayName = @"Gemini";
                    subProvider = @"antigravity"; // 图标用 G
                } else {
                    displayName = [item[@"providerId"] isEqualToString:@"codex"] ? @"Codex" : grpName;
                    if ([item[@"providerId"] isEqualToString:@"codex"]) subProvider = @"codex";
                }

                [quotaFrames addObject:@{
                    @"kind": @"quota",
                    @"provider": subProvider,
                    @"name": displayName,
                    @"window": selected,
                    @"allWindows": grpWindows,
                    @"sortPct": @(sortPct)
                }];
            }
        }
        // 余量最少的渠道先轮播（哪个最少先显示哪个）
        [quotaFrames sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
            return [@([a[@"sortPct"] doubleValue]) compare:@([b[@"sortPct"] doubleValue])];
        }];
        NSDictionary *worst = quotaFrames.firstObject;
        self.worstLimitPercent = worst ? [worst[@"sortPct"] doubleValue] : -1;
        self.worstLimitText = worst ? [NSString stringWithFormat:@"%@ %.0f%%", worst[@"name"], self.worstLimitPercent] : @"";

            NSMutableArray *frames = [NSMutableArray array];
            [frames addObject:@{@"kind": @"usage",
                                 @"tokens": self.latestTokensText ?: @"--",
                                 @"cost": [self costNumberFirst:self.latestCostText]}];
            [frames addObjectsFromArray:quotaFrames];
            self.carouselFrames = frames;
            self.carouselTick = 0;
            [self renderCarouselFrame];
}

// 额度行文案（单行，按约束状态）："5H 86% · 3时20分后重置" / "5H · 1时20分后重置" / "周 · 6天后重置"
// 耗尽状态下百分比无意义，只显示重置倒计时
- (NSString *)quotaLine:(NSDictionary *)window {
    if (![window isKindOfClass:[NSDictionary class]]) return @"";
    NSMutableString *line = [NSMutableString string];
    [line appendFormat:@"%@", [self quotaWindowShortLabel:[window[@"label"] isKindOfClass:[NSString class]] ? window[@"label"] : @"额度"]];
    double pct = [window[@"pct"] isKindOfClass:[NSNumber class]] ? [window[@"pct"] doubleValue] : 0.0;
    if (pct > 0) [line appendFormat:@" %@", [self percentText:pct]];
    double epoch = [window[@"resetEpoch"] isKindOfClass:[NSNumber class]] ? [window[@"resetEpoch"] doubleValue] : 0.0;
    NSString *countdown = [self countdownTextFromEpoch:epoch];
    if (countdown) [line appendFormat:@" · %@", countdown];
    return line;
}

// 每秒渲染当前帧（倒计时逐秒走），每 20 秒切换到下一帧
- (void)carouselTickHandler {
    if (self.displayMode != NativesStatusBarDisplayModeBoth) return;
    self.carouselTick++;
    [self renderCarouselFrame];
}

- (void)renderCarouselFrame {
    NSStatusBarButton *button = self.statusItem.button;
    if (!button) return;

    NSArray *frames = self.carouselFrames;
    if (frames.count == 0) {
        button.image = [self createTemplateIcon];
        button.attributedTitle = [[NSAttributedString alloc] initWithString:@""];
        button.title = [NSString stringWithFormat:@" %@", self.latestDisplayText];
        return;
    }

    NSFont *font = [NSFont menuBarFontOfSize:9.0];
    NSMutableParagraphStyle *paragraph = [NSMutableParagraphStyle new];
    paragraph.alignment = NSTextAlignmentCenter;
    NSDictionary *attrs = @{
        NSFontAttributeName: font,
        NSForegroundColorAttributeName: [NSColor blackColor],
        NSParagraphStyleAttributeName: paragraph
    };

    // 收集全部帧的行文案（倒计时实时计算）：统一画布宽度，切换帧时按钮不抖动
    NSMutableArray *rendered = [NSMutableArray array];
    CGFloat textWidth = 0;
    for (NSDictionary *frame in frames) {
        NSMutableArray *lines = [NSMutableArray array];
        if ([frame[@"kind"] isEqualToString:@"usage"]) {
            [lines addObject:self.latestTokensText ?: @"--"];
            [lines addObject:[self costNumberFirst:self.latestCostText]];
        } else {
            // 额度帧拆两行降低宽度：上行 渠道名+百分比（有余量时），下行 重置倒计时
            NSDictionary *window = frame[@"window"];
            NSString *label = [self quotaWindowShortLabel:window[@"label"]];
            NSString *line1 = [NSString stringWithFormat:@"%@ %@", label, [self percentText:[window[@"pct"] doubleValue]]];
            NSString *line2 = [self compactCountdownFromEpoch:[window[@"resetEpoch"] doubleValue]] ?: @"--";

            [lines addObject:line1];
            [lines addObject:line2];
        }
        if (lines.count == 0) continue;
        for (NSString *l in lines) {
            textWidth = MAX(textWidth, [l sizeWithAttributes:attrs].width);
        }
        [rendered addObject:@[lines, frame[@"kind"] ?: @"", frame[@"provider"] ?: @""]];
    }

    // 整帧绘制成一张紧凑模板图元：画布 22pt 高，图标 14pt 居中，
    // 严格控制总宽（约 50pt，仅相当于 2~3 个字符宽度），避免在顶栏占过长区域
    CGFloat iconSize = 14.0, iconX = 0.0, textX = iconX + iconSize + 2.5, padding = 1.5;
    CGFloat height = 22.0, band = height / 2.0;
    CGFloat width = floor(textX + textWidth + padding);

    NSUInteger index = (self.carouselTick / 20) % rendered.count;
    NSArray *current = rendered[index];
    NSArray *lines = current[0];
    BOOL isUsage = [current[1] isEqualToString:@"usage"];
    NSImage *icon = isUsage ? [self createTemplateIcon] : [self channelIcon:current[2]];

    NSImage *image = [NSImage imageWithSize:NSMakeSize(width, height) flipped:YES drawingHandler:^BOOL(NSRect dst) {
        [icon drawInRect:NSMakeRect(iconX, (height - iconSize) / 2.0, iconSize, iconSize)
                fromRect:NSZeroRect
               operation:NSCompositingOperationSourceOver
                  fraction:1.0
            respectFlipped:YES
                     hints:nil];
        CGFloat textAreaWidth = width - textX - padding;
        CGFloat lineHeight = font.ascender - font.descender;
        // 统一两行布局：上行/下行各自在半区内水平居中、垂直居中
        CGFloat x1 = textX + (textAreaWidth - [lines[0] sizeWithAttributes:attrs].width) / 2.0;
        CGFloat y1 = band / 2.0 - lineHeight / 2.0;
        CGFloat x2 = textX + (textAreaWidth - [lines[1] sizeWithAttributes:attrs].width) / 2.0;
        CGFloat y2 = band + band / 2.0 - lineHeight / 2.0;
        [lines[0] drawAtPoint:NSMakePoint(x1, y1) withAttributes:attrs];
        [lines[1] drawAtPoint:NSMakePoint(x2, y2) withAttributes:attrs];
        return YES;
    }];
    [image setTemplate:YES];

    button.image = image;
    button.imagePosition = NSImageOnly;
    button.title = @"";
    button.attributedTitle = [[NSAttributedString alloc] initWithString:@""];
}

- (NSImage *)channelIcon:(NSString *)providerId {
    static NSMutableDictionary *cache = nil;
    static dispatch_once_t once;
    dispatch_once(&once, ^{ cache = [NSMutableDictionary dictionary]; });

    NSString *key = (providerId ?: @"").lowercaseString;
    NSString *letter = @"•";
    if ([key isEqualToString:@"antigravity"]) letter = @"G";
    else if ([key isEqualToString:@"codex"] || [key isEqualToString:@"chatgpt"]) letter = @"O";
    else if ([key isEqualToString:@"claude"] || [key isEqualToString:@"anthropic"]) letter = @"C";
    else if ([key isEqualToString:@"kimi"]) letter = @"K";
    else if ([key isEqualToString:@"xai"] || [key isEqualToString:@"grok"]) letter = @"X";

    NSImage *cached = cache[letter];
    if (cached) return cached;

    NSImage *img = [NSImage imageWithSize:NSMakeSize(16, 16) flipped:NO drawingHandler:^BOOL(NSRect dstRect) {
        NSDictionary *attrs = @{
            NSFontAttributeName: [NSFont systemFontOfSize:12.5 weight:NSFontWeightSemibold],
            NSForegroundColorAttributeName: [NSColor blackColor]
        };
        NSSize size = [letter sizeWithAttributes:attrs];
        [letter drawAtPoint:NSMakePoint((dstRect.size.width - size.width) / 2.0, (dstRect.size.height - size.height) / 2.0)
             withAttributes:attrs];
        return YES;
    }];
    [img setTemplate:YES];
    cache[letter] = img;
    return img;
}

- (void)updateWithStateJson:(NSString *)jsonText {
    if (!jsonText || jsonText.length == 0) return;

    NSData *data = [jsonText dataUsingEncoding:NSUTF8StringEncoding];
    if (!data) return;

    NSError *error = nil;
    NSDictionary *json = [NSJSONSerialization JSONObjectWithData:data options:0 error:&error];
    if (error || ![json isKindOfClass:[NSDictionary class]]) return;

    NSString *disp = json[@"displayText"];
    if ([disp isKindOfClass:[NSString class]]) {
        self.latestDisplayText = disp;
        NSArray *parts = [disp componentsSeparatedByString:@" · "];
        if (parts.count >= 2) {
            self.latestTokensText = parts[0];
            self.latestCostText = parts[1];
        } else {
            self.latestTokensText = disp;
            self.latestCostText = @"--";
        }
    }

    NSString *tip = json[@"tooltip"];
    if ([tip isKindOfClass:[NSString class]]) {
        self.latestTooltip = tip;
    }

    self.worstLimitText = @"";
    self.worstLimitPercent = -1;
    NSDictionary *worst = json[@"worstLimit"];
    if ([worst isKindOfClass:[NSDictionary class]]) {
        id rem = worst[@"remainingPercent"];
        NSString *prov = worst[@"providerId"];
        if ([rem isKindOfClass:NSNumber.class] && isfinite([rem doubleValue]) && [rem doubleValue] >= 0 && [rem doubleValue] <= 100 && [prov isKindOfClass:[NSString class]]) {
            self.worstLimitText = [NSString stringWithFormat:@"%@ %.0f%%", prov, [rem doubleValue]];
            double pct = [rem doubleValue];
            self.worstLimitPercent = (pct >= 0 && pct <= 100) ? pct : -1;
        }
    } else {
        self.worstLimitText = @"";
        self.worstLimitPercent = -1;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        [self refreshButtonUI];
        [self buildMenu];
    });
}

- (void)fetchAndUpdateState {
    if (self.usageFetchInFlight || self.shuttingDown) return;
    self.usageFetchInFlight = YES;
    NSUInteger generation = ++self.usageGeneration;
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSDictionary *state = [self readUsageState];
        dispatch_async(dispatch_get_main_queue(), ^{
            self.usageFetchInFlight = NO;
            if (self.shuttingDown || generation != self.usageGeneration) return;
            NSDictionary *display = state ?: @{@"displayText": @"-- · --", @"tooltip": @"用量数据暂不可用", @"worstLimit": NSNull.null};
            NSData *data = [NSJSONSerialization dataWithJSONObject:display options:0 error:nil];
            if (data) [self updateWithStateJson:[[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding]];
            [self rebuildQuotaFrames];
        });
    });
}

- (NSDictionary *)readUsageState {
    [self ensureModelHostAlive];
    return FetchCompactStatusBarState();
}

- (NSDictionary *)readPanelState {
    [self ensureModelHostAlive];
    return FetchStatusBarState();
}

- (void)openTargetApp:(NSString *)appId {
    NSURL *url = NativesStatusBarTargetURL(appId);
    if (!url) return;
    if (!NativesStatusBarUsesWeb()) {
        [NSWorkspace.sharedWorkspace openURL:url];
        return;
    }
    NSString *core;
#ifdef SOURCE_ROOT
    core = [NSString stringWithFormat:@"%s/native-file-host", SOURCE_ROOT];
#else
    core = [NSBundle.mainBundle.executablePath.stringByDeletingLastPathComponent stringByAppendingPathComponent:@"native-file-host"];
#endif
    NSTask *task = [NSTask new];
    task.executableURL = [NSURL fileURLWithPath:core];
    task.arguments = @[@"--launcher-web", appId];
    task.standardOutput = NSFileHandle.fileHandleWithNullDevice;
    task.standardError = NSFileHandle.fileHandleWithNullDevice;
    task.terminationHandler = ^(NSTask *completed) {
        if (completed.terminationStatus == 0) return;
        dispatch_async(dispatch_get_main_queue(), ^{ NSBeep(); });
    };
    NSError *error;
    if (![task launchAndReturnError:&error]) NSBeep();
}

- (void)actOpenTokenUsage {
    [self openTargetApp:@"tokenusage"];
}

- (void)actOpenSpace {
    [self openTargetApp:@"space"];
}

- (void)actOpenFiles {
    [self openTargetApp:@"files"];
}

// 显式关闭仅属于顶栏的资源：worker stdin 写端与 stdout/stderr drain handler。
// 顶栏退出 → EOF → worker 按自身 resident 设置决定退出；不终止其他客户端，
// 不清理其他会话的实例（生命周期整改 T4/L4）。
- (void)shutdownModelHostConnection {
    self.shuttingDown = YES;
    [self.modelHostSpawnLock lock];
    @try {
        self.shuttingDown = YES;
        [self.refreshTimer invalidate];
        [self.carouselTimer invalidate];
        NSTask *task = self.modelHostTask;
        if (task && task.isRunning) {
            [self.modelHostStdinPipe.fileHandleForWriting closeFile];
        }
        if (task) {
            ((NSPipe *)task.standardOutput).fileHandleForReading.readabilityHandler = nil;
            ((NSPipe *)task.standardError).fileHandleForReading.readabilityHandler = nil;
        }
    } @finally {
        [self.modelHostSpawnLock unlock];
    }
}

- (void)actQuit {
    [self shutdownModelHostConnection];
    [NSApp terminate:nil];
}

@end
