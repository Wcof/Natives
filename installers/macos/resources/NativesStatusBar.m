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
// 启动时清掉同二进制旧实例再拉起；退出时（atexit，覆盖菜单退出/关窗/交接
// 完成等全部正常退出路径）再兜底清扫一轮。匹配口径 = 可执行文件路径完全一致
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
        kill((pid_t)pidNum.intValue, SIGKILL);
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
// 最近一次手动刷新获得的 Codex 真实额度（内存驻留；面板重开时复用，不重复请求上游）
@property (nonatomic, copy) NSDictionary *lastCodexLive;
// 顶栏双行轮播：帧序列（用量帧 + 各渠道额度帧）、1 秒渲染 / 3 秒切换
@property (nonatomic, copy) NSArray *carouselFrames;
@property (nonatomic, assign) NSUInteger carouselTick;
@property (nonatomic, strong) NSTimer *carouselTimer;
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
        _runtimeBaseUrl = @"http://127.0.0.1:8765"; // 默认 loopback 运行时地址
        _latestDisplayText = @"--";
        _latestTokensText = @"--";
        _latestCostText = @"--";
        _latestTooltip = @"Token Usage";
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

    // 顶栏双行轮播：每秒渲染（额度倒计时走秒），每 3 秒切换一帧
    self.carouselTimer = [NSTimer timerWithTimeInterval:1.0
                                                 target:self
                                               selector:@selector(carouselTickHandler)
                                               userInfo:nil
                                                repeats:YES];
    [[NSRunLoop mainRunLoop] addTimer:self.carouselTimer forMode:NSRunLoopCommonModes];
    [self rebuildQuotaFrames];
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
    [nc addObserver:self selector:@selector(panelResignedKey) name:NSWindowDidResignKeyNotification object:self.popoverPanel];
    return self.popoverPanel;
}

- (void)panelResignedKey {
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
        [self.popoverPanel orderOut:nil];
        return;
    }

    NSPanel *panel = [self ensurePanel];

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
}

- (void)webView:(WKWebView *)webView didFinishNavigation:(WKNavigation *)navigation {
    [self fetchPanelData];
}

- (void)panelClosed {
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

    dispatch_group_t group = dispatch_group_create();

    NSURLSessionConfiguration *config = [NSURLSessionConfiguration ephemeralSessionConfiguration];
    config.timeoutIntervalForRequest = 1.5;
    NSURLSession *session = [NSURLSession sessionWithConfiguration:config];

    dispatch_group_enter(group);
    // 面板状态一律走本地聚合（tokenusage 库 + ZCode/Codex/pi 直连会话扫描），
    // 这是服务端 tray/state（仅 tokenusage 库）的超集；扫描可能读数百个会话文件，
    // 放后台线程执行，结果回主线程投递
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSDictionary *state = FetchStateFromLocalSqlite();
        dispatch_async(dispatch_get_main_queue(), ^{
            if (self.webView) {
                if (state) {
                    NSData *stateData = [NSJSONSerialization dataWithJSONObject:state options:0 error:nil];
                    if (stateData) {
                        [self updateWithStateJson:[[NSString alloc] initWithData:stateData encoding:NSUTF8StringEncoding]];
                    }
                    id panel = state[@"panel"];
                    if (panel) {
                        [self postToPanel:@{@"type": @"panel", @"panel": panel}];
                    }
                    [self postToPanel:@{@"type": @"limits", @"items": FetchLimitsFromLocalSqlite() ?: @[]}];
                } else {
                    NSDictionary *emptyPanel = @{
                        @"today": @{@"totalTokens": @0, @"costUsd": @0.0},
                        @"thisWeek": @{@"totalTokens": @0, @"costUsd": @0.0},
                        @"thisMonth": @{@"totalTokens": @0, @"costUsd": @0.0},
                        @"allTime": @{@"totalTokens": @0, @"costUsd": @0.0},
                        @"tools": @[],
                        @"toolsByPeriod": @{@"today": @[], @"thisWeek": @[], @"thisMonth": @[], @"allTime": @[]},
                        @"sessions": @[],
                        @"trends": @[]
                    };
                    [self postToPanel:@{@"type": @"panel", @"panel": emptyPanel}];
                    [self postToPanel:@{@"type": @"limits", @"items": @[]}];
                }
            }
            dispatch_group_leave(group);
        });
    });

    dispatch_group_enter(group);
    [self fetchLimitsWithCompletion:^(NSArray *items) {
        if (self.webView) {
            [self postToPanel:@{@"type": @"limits", @"items": items ?: @[]}];
        }
        dispatch_group_leave(group);
    } live:live];

    dispatch_group_notify(group, dispatch_get_main_queue(), ^{
        [session finishTasksAndInvalidate];
        if (completion) completion();
        // 额度数据（Codex 实时结果 / 扩展写入的缓存）落地后重建顶栏轮播帧
        [self rebuildQuotaFrames];
    });
}

// 账号额度解析（后台线程）：本地 limits_cache 真实行 + 账号身份为基底。
// Codex 真实额度仅在 live=YES（用户手动刷新）时实时请求上游；请求结果驻留内存，
// 面板重开与状态轮询一律复用最近一次结果，不重复请求（避免高频请求触发账号风控）。
// 无凭据/请求失败/尚未手动刷新时不产生任何占位数据，如实保留空窗口。
- (void)fetchLimitsWithCompletion:(void (^)(NSArray *items))completion live:(BOOL)live {
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSArray *base = FetchLimitsFromLocalSqlite();
        NSDictionary *codexLive = nil;
        if (live) {
            codexLive = FetchCodexUsageLive();
            if (codexLive) self.lastCodexLive = codexLive;
        } else {
            codexLive = self.lastCodexLive;
        }
        NSArray *items = MergeCodexLiveUsage(base, codexLive);
        dispatch_async(dispatch_get_main_queue(), ^{
            completion(items ?: @[]);
        });
    });
}

// 刷新动作（仅由用户点击触发）：重拉面板状态，并实时请求 Codex 额度上游，全部落地后
// 通知面板停止转圈。额度上游请求只由本动作发起；面板打开与 30s 状态轮询均不请求。
// 运行时不可达时本地 SQLite 直读兜底。顶栏进程无 Bearer 会话（ADR-0032 仅豁免
// 只读路由），不请求会话制的额度回源端点。
- (void)performPanelRefresh {
    if (!self.webView) return;
    [self fetchPanelDataLive:YES completion:^{
        [self postToPanel:@{@"type": @"refreshDone"}];
    }];
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
    [self.popoverPanel orderOut:nil];
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
        @{@"title": @"仅今日 Token (1.25M)", @"mode": @(NativesStatusBarDisplayModeTokens)},
        @{@"title": @"仅今日费用 ($3.42)", @"mode": @(NativesStatusBarDisplayModeCost)},
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
    if (seconds <= 0) return @"0S";
    long long sec = (long long)seconds;
    if (sec < 60) return [NSString stringWithFormat:@"%lldS", sec];
    if (sec < 3600) return [NSString stringWithFormat:@"%lldM%lldS", sec / 60, sec % 60];
    if (sec < 86400) return [NSString stringWithFormat:@"%lldH%lldM", sec / 3600, (sec % 3600) / 60];
    return [NSString stringWithFormat:@"%lldD%lldH", sec / 86400, (sec % 86400) / 3600];
}

// 窗口类型缩写：5小时额度→5H、周额度→周、日额度→日、月额度→月
- (NSString *)quotaWindowShortLabel:(NSString *)label {
    NSString *raw = label ?: @"";
    if ([raw containsString:@"5小时"]) return @"5H";
    if ([raw containsString:@"周"]) return @"周";
    if ([raw containsString:@"日"]) return @"日";
    if ([raw containsString:@"月"]) return @"月";
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
    if (pct == (double)llround(pct)) return [NSString stringWithFormat:@"%.0f%%", pct];
    return [NSString stringWithFormat:@"%.1f%%", pct];
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
        if ([label containsString:@"5小时"]) {
            if (pct < pct5h) { pct5h = pct; best5h = w; }
        } else if ([label containsString:@"周"]) {
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
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSArray *items = FetchLimitsFromLocalSqlite();
        items = MergeCodexLiveUsage(items, self.lastCodexLive);

        NSMutableArray *quotaFrames = [NSMutableArray array];
        for (NSDictionary *item in items) {
            if (![item isKindOfClass:[NSDictionary class]]) continue;
            NSArray *windows = [item[@"windows"] isKindOfClass:[NSArray class]] ? item[@"windows"] : @[];
            if (windows.count == 0) continue; // 无额度数据的渠道直接跳过
            NSDictionary *selected = [self selectDisplayWindow:windows];
            if (!selected) continue;

            double sortPct = 101.0;
            if ([selected[@"pct"] isKindOfClass:[NSNumber class]]) sortPct = [selected[@"pct"] doubleValue];

            [quotaFrames addObject:@{
                @"kind": @"quota",
                @"provider": item[@"providerId"] ?: @"",
                @"window": selected,
                @"sortPct": @(sortPct)
            }];
        }
        // 余量最少的渠道先轮播（哪个最少先显示哪个）
        [quotaFrames sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
            return [@([a[@"sortPct"] doubleValue]) compare:@([b[@"sortPct"] doubleValue])];
        }];

        dispatch_async(dispatch_get_main_queue(), ^{
            NSMutableArray *frames = [NSMutableArray array];
            [frames addObject:@{@"kind": @"usage",
                                 @"tokens": self.latestTokensText ?: @"--",
                                 @"cost": [self costNumberFirst:self.latestCostText]}];
            [frames addObjectsFromArray:quotaFrames];
            self.carouselFrames = frames;
            self.carouselTick = 0;
            [self renderCarouselFrame];
        });
    });
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
            // 额度帧拆两行降低宽度：上行 窗口简称+百分比（有余量时），下行 重置倒计时
            NSDictionary *w = [frame[@"window"] isKindOfClass:[NSDictionary class]] ? frame[@"window"] : @{};
            NSString *shortLabel = [self quotaWindowShortLabel:[w[@"label"] isKindOfClass:[NSString class]] ? w[@"label"] : @"额度"];
            double pct = [w[@"pct"] isKindOfClass:[NSNumber class]] ? [w[@"pct"] doubleValue] : 0.0;
            if (pct > 0) {
                [lines addObject:[NSString stringWithFormat:@"%@ %@", shortLabel, [self percentText:pct]]];
            } else {
                [lines addObject:shortLabel]; // 耗尽时百分比无意义，不再显示
            }
            double epoch = [w[@"resetEpoch"] isKindOfClass:[NSNumber class]] ? [w[@"resetEpoch"] doubleValue] : 0.0;
            NSString *countdown = [self compactCountdownFromEpoch:epoch];
            [lines addObject:countdown ?: @"--"];
        }
        if (lines.count == 0) continue;
        for (NSString *l in lines) {
            textWidth = MAX(textWidth, [l sizeWithAttributes:attrs].width);
        }
        [rendered addObject:@[lines, frame[@"kind"] ?: @"", frame[@"provider"] ?: @""]];
    }

    // 整帧绘制成一张模板图元：画布 22pt 高；用量帧占上下两半，额度帧单行垂直居中，
    // 渠道图标 16pt 垂直居中——垂直居中由几何直接保证，不再依赖按钮文本排版
    CGFloat iconSize = 16.0, iconX = 1.0, textX = iconX + iconSize + 2.0, padding = 3.0;
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
            self.latestCostText = disp;
        }
    }

    NSString *tip = json[@"tooltip"];
    if ([tip isKindOfClass:[NSString class]]) {
        self.latestTooltip = tip;
    }

    NSDictionary *worst = json[@"worstLimit"];
    if ([worst isKindOfClass:[NSDictionary class]]) {
        id rem = worst[@"remainingPercent"];
        NSString *prov = worst[@"providerId"];
        if (rem && [prov isKindOfClass:[NSString class]]) {
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
    NSURL *url = [NSURL URLWithString:[NSString stringWithFormat:@"%@/api/tray/state", self.runtimeBaseUrl]];
    if (!url) return;

    NSURLSessionConfiguration *config = [NSURLSessionConfiguration ephemeralSessionConfiguration];
    config.timeoutIntervalForRequest = 2.0;
    NSURLSession *session = [NSURLSession sessionWithConfiguration:config];

    NSURLSessionDataTask *task = [session dataTaskWithURL:url completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
        if (!error && data) {
            NSString *text = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
            [self updateWithStateJson:text];
        } else {
            NSDictionary *localState = FetchStateFromLocalSqlite();
            if (localState) {
                NSData *localData = [NSJSONSerialization dataWithJSONObject:localState options:0 error:nil];
                if (localData) {
                    NSString *localText = [[NSString alloc] initWithData:localData encoding:NSUTF8StringEncoding];
                    [self updateWithStateJson:localText];
                }
            }
        }
        // 状态更新后重建顶栏轮播帧（今日用量 + 额度缓存有变）
        dispatch_async(dispatch_get_main_queue(), ^{
            [self rebuildQuotaFrames];
        });
    }];
    [task resume];
    [session finishTasksAndInvalidate];
}

- (void)openTargetApp:(NSString *)appId {
    NSString *route = @"app.html";
    if ([appId isEqualToString:@"space"]) {
        route = @"space.html";
    } else if ([appId isEqualToString:@"files"]) {
        route = @"files.html";
    } else if (appId.length > 0) {
        route = [NSString stringWithFormat:@"app.html?app=%@", appId];
    }

    NSString *cmd = [NSString stringWithFormat:@"open \"chrome-extension://kooobajbofcajlblcannmdckihiejdec/%@\"", route];
    system([cmd UTF8String]);
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

- (void)actQuit {
    [NSApp terminate:nil];
}

@end
