// NativesStatusBar.m: macOS 顶部系统菜单栏（NSStatusItem）常驻“标签栏”实现

#import "NativesStatusBar.h"
#import <WebKit/WebKit.h>
#import <sqlite3.h>

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
@end

#pragma mark - 强调色进度条

// 自绘额度条：中性轨道 + Token Monitor --accent（#b7ead4）填充；
// percent < 0 表示无数据，整条隐藏。
@interface MeterBarView : NSView
@property (nonatomic, assign) double percent; // 0~100，无数据为 -1
@end

@implementation MeterBarView

- (void)drawRect:(NSRect)dirtyRect {
    if (self.percent < 0) return;

    CGFloat h = self.bounds.size.height;
    NSBezierPath *track = [NSBezierPath bezierPathWithRoundedRect:self.bounds xRadius:h / 2.0 yRadius:h / 2.0];
    [[NSColor colorWithWhite:1.0 alpha:0.14] setFill];
    [track fill];

    CGFloat clamped = MAX(0.0, MIN(100.0, self.percent));
    if (clamped <= 0) return;
    CGFloat fillW = MAX(h, self.bounds.size.width * clamped / 100.0);
    NSBezierPath *fill = [NSBezierPath bezierPathWithRoundedRect:NSMakeRect(0, 0, fillW, h) xRadius:h / 2.0 yRadius:h / 2.0];
    // 低余量 (<20%) 转警示红，与 Token Monitor 的语义色一致
    NSColor *accent = (clamped < 20.0)
        ? [NSColor colorWithSRGBRed:0.957 green:0.467 blue:0.533 alpha:1.0] // #f47788
        : [NSColor colorWithSRGBRed:0.718 green:0.918 blue:0.831 alpha:1.0]; // #b7ead4
    [accent setFill];
    [fill fill];
}

- (BOOL)isHiddenOrHasHiddenAncestor {
    return self.percent < 0 || [super isHiddenOrHasHiddenAncestor];
}

- (NSSize)intrinsicContentSize {
    return NSMakeSize(NSViewNoIntrinsicMetric, 5.0);
}

@end

#pragma mark - Token Monitor 风格头部数据卡

// 深色玻璃卡片（HUD 材质观感）+ 等宽数字 + 强调色进度条，
// 视觉对齐 Token Monitor 弹窗样式。
@interface TokenMonitorHeaderView : NSView
@property (nonatomic, copy) NSString *displayText;
@property (nonatomic, copy) NSString *limitText;
@property (nonatomic, assign) double limitPercent; // 0~100，无数据为 -1
- (instancetype)initWithFrame:(NSRect)frame
                 displayText:(NSString *)displayText
                  limitText:(NSString *)limitText
               limitPercent:(double)limitPercent;
@end

@implementation TokenMonitorHeaderView

- (instancetype)initWithFrame:(NSRect)frame
                 displayText:(NSString *)displayText
                  limitText:(NSString *)limitText
               limitPercent:(double)limitPercent {
    self = [super initWithFrame:frame];
    if (self) {
        _displayText = [displayText copy] ?: @"--";
        _limitText = [limitText copy];
        _limitPercent = limitPercent;

        // 毛玻璃底：HUD 深色材质，深浅色菜单栏下均成立
        NSVisualEffectView *glass = [[NSVisualEffectView alloc] initWithFrame:self.bounds];
        glass.material = NSVisualEffectMaterialHUDWindow;
        glass.blendingMode = NSVisualEffectBlendingModeWithinWindow;
        glass.state = NSVisualEffectStateActive;
        glass.wantsLayer = YES;
        glass.layer.cornerRadius = 10.0;
        glass.layer.masksToBounds = YES;
        glass.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        [self addSubview:glass];

        CGFloat w = frame.size.width;
        CGFloat x = 12.0;

        // 标题行：矢量闪电图标 + 「今日消耗」小标签（不使用 emoji）
        NSImageView *boltView = [[NSImageView alloc] initWithFrame:NSMakeRect(x, w >= 240 ? 64.0 : 58.0, 11.0, 13.0)];
        boltView.image = [self boltImage];
        boltView.imageScaling = NSImageScaleProportionallyDown;
        boltView.contentTintColor = [NSColor secondaryLabelColor];
        [self addSubview:boltView];

        [self addLabelWithString:@"今日消耗"
                            font:[NSFont menuFontOfSize:12]
                           color:[NSColor secondaryLabelColor]
                              at:NSMakePoint(x + 16.0, w >= 240 ? 62.0 : 56.0)];

        // 大号等宽数字：Token · 费用
        [self addLabelWithString:_displayText
                            font:[NSFont monospacedDigitSystemFontOfSize:17 weight:NSFontWeightMedium]
                           color:[NSColor labelColor]
                              at:NSMakePoint(x, w >= 240 ? 38.0 : 32.0)];

        // 进度条：自绘轨道 + 强调色填充（Token Monitor --accent 薄荷绿），无数据时隐藏
        MeterBarView *meter = [[MeterBarView alloc] initWithFrame:NSMakeRect(x, 20.0, w - 2 * x, 5.0)];
        meter.percent = _limitPercent;
        meter.autoresizingMask = NSViewWidthSizable;
        [self addSubview:meter];

        NSString *limitCaption = _limitPercent >= 0
            ? [NSString stringWithFormat:@"最低额度 %.0f%%%@", _limitPercent, (_limitText.length > 0 ? [NSString stringWithFormat:@" · %@", _limitText] : @"")]
            : @"暂无额度数据";
        [self addLabelWithString:limitCaption
                            font:[NSFont monospacedDigitSystemFontOfSize:10 weight:NSFontWeightRegular]
                           color:[NSColor tertiaryLabelColor]
                              at:NSMakePoint(x, 4.0)];
    }
    return self;
}

// 矢量闪电图元（与顶栏模板图标同形），通过 contentTintColor 着色
- (NSImage *)boltImage {
    NSImage *img = [NSImage imageWithSize:NSMakeSize(11, 13) flipped:NO drawingHandler:^BOOL(NSRect dstRect) {
        NSBezierPath *path = [NSBezierPath bezierPath];
        [path moveToPoint:NSMakePoint(7.0, 13.0)];
        [path lineToPoint:NSMakePoint(2.0, 7.0)];
        [path lineToPoint:NSMakePoint(5.5, 7.0)];
        [path lineToPoint:NSMakePoint(4.5, 0.0)];
        [path lineToPoint:NSMakePoint(9.5, 6.5)];
        [path lineToPoint:NSMakePoint(6.0, 6.5)];
        [path closePath];
        [[NSColor blackColor] setFill];
        [path fill];
        return YES;
    }];
    return img;
}

- (NSTextField *)addLabelWithString:(NSString *)str
                               font:(NSFont *)font
                              color:(NSColor *)color
                                 at:(NSPoint)origin {
    NSTextField *tf = [NSTextField labelWithString:str];
    tf.font = font;
    tf.textColor = color;
    tf.frame = NSMakeRect(origin.x, origin.y, self.bounds.size.width - origin.x - 12.0, ceilf(font.maximumAdvancement.height) + 4.0);
    tf.autoresizingMask = NSViewWidthSizable;
    [self addSubview:tf];
    return tf;
}

- (NSSize)intrinsicContentSize {
    return NSMakeSize(240, self.limitPercent >= 0 ? 78 : 60);
}

@end

// borderless 面板默认 canBecomeKeyWindow=NO，makeKeyAndOrderFront 不生效，
// 面板无法成为 key 窗口导致显示异常；必须子类化放开。
@interface PopoverPanel : NSPanel
@end

@implementation PopoverPanel
- (BOOL)canBecomeKeyWindow {
    return YES;
}
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

#pragma mark - Token Monitor 弹窗面板（无边框 NSPanel + WKWebView）

// 复刻 token-monitor 顶栏弹窗：深色 HUD 玻璃、等宽数字、强调色额度条。
// 数据由原生层拉取后经 window.postMessage 注入，避免页面直接跨域请求。
- (NSString *)panelHTML {
    return @"<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">"
    @"<style>"
    // ===== 设计变量：对齐 token-monitor styles.css :root =====
    @":root{"
    @"color-scheme:dark;"
    @"--glass-rgb:32,36,44;"
    @"--glass-alpha:0.75;"
    @"--glass:rgba(var(--glass-rgb),var(--glass-alpha));"
    @"--glass-filter:blur(40px) saturate(140%);"
    @"--line:rgba(255,255,255,0.09);"
    @"--line-strong:rgba(255,255,255,0.18);"
    @"--panel-rgb:18,22,29;"
    @"--sunken-rgb:8,11,16;"
    @"--text:#eef5fb;"
    @"--muted:#8e9aa8;"
    @"--accent:#b7ead4;"
    @"--accent-rgb:183,234,212;"
    @"--number:#f3fbf7;"
    @"--blue-rgb:115,189,245;"
    @"--blue:rgb(var(--blue-rgb));"
    @"--orange:#f4a073;"
    @"--red:#f47788;"
    @"--yellow:#f1d973;"
    @"--purple:#b394f4;"
    @"--ui-font:-apple-system,BlinkMacSystemFont,'SF Pro Text','Segoe UI',Roboto,Helvetica,sans-serif;"
    @"--display-font:-apple-system,BlinkMacSystemFont,'SF Pro Display','Segoe UI',sans-serif;"
    @"--mono-font:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,'Liberation Mono',monospace;"
    @"}"
    @"*{box-sizing:border-box;margin:0;padding:0}"
    @"html,body{width:100%;height:100%;background:transparent;font-family:var(--ui-font);color:var(--text);"
    @"user-select:none;-webkit-user-select:none;overflow:hidden;font-size:11px}"
    @"body{width:360px;height:560px}"
    @"/* ===== 毛玻璃主容器 ===== */"
    @".shell{width:100%;height:100%;padding:14px;display:flex;flex-direction:column;gap:10px;"
    @"background:linear-gradient(180deg,rgba(36,42,52,0.78) 0%,rgba(20,24,31,0.72) 100%);"
    @"border:1px solid rgba(255,255,255,0.12);border-radius:14px;"
    @"box-shadow:0 24px 60px rgba(0,0,0,0.55),inset 0 1px 0 rgba(255,255,255,0.1);"
    @"-webkit-backdrop-filter:var(--glass-filter);backdrop-filter:var(--glass-filter);"
    @"position:relative}"
    @"/* ===== 头部：Brand + Tabs ===== */"
    @".head{display:flex;align-items:center;justify-content:space-between;flex-shrink:0;padding:0 2px 2px}"
    @".head .brand-box{display:flex;align-items:center;gap:6px}"
    @".head .brand{display:inline-flex;align-items:center;gap:6px;font-size:13px;font-weight:700;color:var(--text);letter-spacing:0.3px}"
    @".head .brand svg{width:14px;height:14px;fill:var(--accent);filter:drop-shadow(0 0 5px rgba(var(--accent-rgb),0.5));flex:none}"
    @".live-dot{width:5px;height:5px;border-radius:50%;background:#22c55e;box-shadow:0 0 6px rgba(34,197,94,0.7);display:inline-block;margin-left:2px}"
    @".tabs{display:flex;gap:2px;background:rgba(0,0,0,0.22);border:1px solid var(--line);border-radius:8px;padding:2px}"
    @".tabs button{font-family:var(--ui-font);font-size:10px;font-weight:500;color:var(--muted);background:transparent;"
    @"border:0;border-radius:6px;padding:3px 9px;cursor:pointer;letter-spacing:0.4px;transition:all 140ms ease}"
    @".tabs button:hover{color:var(--text)}"
    @".tabs button.active{color:var(--text);font-weight:600;background:rgba(255,255,255,0.1);box-shadow:0 1px 3px rgba(0,0,0,0.25)}"
    @"/* ===== 滚动内容区 ===== */"
    @".content-scroll{flex:1;min-height:0;overflow-y:auto;overflow-x:hidden;display:flex;flex-direction:column;gap:10px;"
    @"padding-right:1px;scrollbar-width:none;-ms-overflow-style:none}"
    @".content-scroll::-webkit-scrollbar{width:0;height:0}"
    @"/* ===== 总额看板 ===== */"
    @".total-panel{flex-shrink:0;background:linear-gradient(180deg,rgba(255,255,255,0.045) 0%,rgba(255,255,255,0.015) 100%),rgba(var(--panel-rgb),0.65);"
    @"border:1px solid var(--line);border-radius:12px;padding:12px 14px;display:flex;justify-content:space-between;align-items:flex-end;"
    @"box-shadow:inset 0 1px 0 rgba(255,255,255,0.08),0 3px 8px rgba(0,0,0,0.15)}"
    @".total-panel .cap{font-size:10px;font-weight:500;color:var(--muted);letter-spacing:0.5px;margin-bottom:4px}"
    @".total-panel .num{font-size:27px;font-weight:600;color:var(--number);font-family:var(--display-font);"
    @"font-variant-numeric:tabular-nums;letter-spacing:0.3px;line-height:1.1}"
    @".total-panel .sub{font-size:11.5px;font-weight:500;color:var(--accent);font-family:var(--mono-font);margin-top:3px;font-variant-numeric:tabular-nums}"
    @".total-panel .right{text-align:right}"
    @".total-panel .right .num{font-size:15px;color:var(--text)}"
    @".total-panel .right .sub{color:var(--muted);font-weight:400;font-size:10.5px}"
    @"/* ===== 分区卡片 ===== */"
    @".section{flex-shrink:0;background:rgba(var(--panel-rgb),0.42);border:1px solid var(--line);"
    @"border-radius:10px;padding:10px 12px;box-shadow:inset 0 1px 0 rgba(255,255,255,0.03)}"
    @".section h3{font-size:10px;color:var(--muted);font-weight:600;"
    @"letter-spacing:0.8px;text-transform:uppercase;margin-bottom:8px;"
    @"display:flex;justify-content:space-between;align-items:center}"
    @".section h3 .more{font-size:10px;color:var(--muted);font-weight:400;text-transform:none;letter-spacing:0}"
    @"/* ===== 工具分解行 ===== */"
    @".tool-row{display:flex;align-items:center;gap:8px;padding:3.5px 0;font-size:11px}"
    @".tool-row .dot{width:7px;height:7px;border-radius:50%;flex:none;box-shadow:0 0 5px currentColor}"
    @".tool-row .name{width:88px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;color:var(--text);font-weight:500}"
    @".tool-row .bar{flex:1;height:4.5px;border-radius:2.5px;background:rgba(255,255,255,0.08);overflow:hidden}"
    @".tool-row .bar>i{display:block;height:100%;border-radius:2.5px;background:var(--blue);transition:width 0.3s ease}"
    @".tool-row .val{width:82px;text-align:right;color:var(--muted);font-family:var(--mono-font);font-size:10.5px;font-variant-numeric:tabular-nums;flex:none}"
    @"/* ===== 额度条行 ===== */"
    @".limit-row{display:flex;align-items:center;gap:8px;padding:3.5px 0;font-size:11px}"
    @".limit-row .prov{width:80px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;color:var(--text);font-weight:500}"
    @".limit-row .pct{width:36px;text-align:right;color:var(--muted);font-family:var(--mono-font);font-size:10.5px;font-variant-numeric:tabular-nums}"
    @".meter{flex:1;height:5px;border-radius:2.5px;background:rgba(255,255,255,0.1);overflow:hidden}"
    @".meter>i{display:block;height:100%;border-radius:2.5px;background:var(--accent);transition:width 0.3s ease}"
    @".meter.low>i{background:var(--red)}"
    @".meter.mid>i{background:var(--yellow)}"
    @"/* ===== 会话行 ===== */"
    @".sess-row{display:flex;align-items:center;gap:8px;padding:3.5px 0;font-size:11px}"
    @".sess-row .name{flex:1;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;color:var(--text);font-family:var(--mono-font);font-size:10.5px}"
    @".sess-row .meta{color:var(--muted);font-family:var(--mono-font);font-size:10.5px;font-variant-numeric:tabular-nums;flex:none}"
    @"/* ===== 趋势柱状 ===== */"
    @".trend{display:flex;align-items:flex-end;gap:6px;height:48px;padding:4px 2px 0}"
    @".trend .col{flex:1;display:flex;flex-direction:column;align-items:center;gap:3px;height:100%;justify-content:flex-end}"
    @".trend .col>i{display:block;width:100%;max-width:22px;border-radius:3px 3px 0 0;background:linear-gradient(180deg,var(--blue) 0%,rgba(var(--blue-rgb),0.6) 100%);min-height:3px;transition:height 0.3s ease}"
    @".trend .col.today>i{background:linear-gradient(180deg,var(--accent) 0%,rgba(var(--accent-rgb),0.6) 100%)}"
    @".trend .col span{font-size:8.5px;font-family:var(--mono-font);color:var(--muted);white-space:nowrap}"
    @"/* ===== 空态 ===== */"
    @".empty{font-size:11px;color:var(--muted);padding:6px 0;text-align:center}"
    @"/* ===== 底部动作行 ===== */"
    @".actions{flex-shrink:0;display:flex;gap:8px;padding-top:4px;border-top:1px solid rgba(255,255,255,0.08)}"
    @".actions button{flex:1;font-family:var(--ui-font);font-size:11px;font-weight:500;color:var(--text);"
    @"background:rgba(255,255,255,0.045);border:1px solid var(--line);border-radius:8px;"
    @"padding:7px 0;cursor:pointer;transition:all 140ms ease;box-shadow:inset 0 1px 0 rgba(255,255,255,0.05)}"
    @".actions button:hover{border-color:var(--line-strong);background:rgba(255,255,255,0.08);transform:translateY(-1px)}"
    @"</style></head><body><div class=\"shell\">"
    // 头部：brand（Natives 闪电 logo + 状态绿点）+ 周期页签（中文：天/周/月/总）
    @"<div class=\"head\"><div class=\"brand-box\"><span class=\"brand\"><svg viewBox=\"0 0 24 24\"><path d=\"m13 2-8 12h6l-1 8 9-13h-6z\"/></svg>Natives</span><span class=\"live-dot\" title=\"Live stream connected\"></span></div>"
    @"<nav class=\"tabs\" id=\"tabs\">"
    @"<button data-p=\"today\" class=\"active\">天</button>"
    @"<button data-p=\"week\">周</button>"
    @"<button data-p=\"month\">月</button>"
    @"<button data-p=\"all\">总</button>"
    @"</nav></div>"
    // 滚动区域开始
    @"<div class=\"content-scroll\">"
    // 总额看板：主数字随页签切换，右侧累计/会话副信息
    @"<div class=\"total-panel\"><div>"
    @"<div class=\"cap\" id=\"tCap\">今日消耗</div>"
    @"<div class=\"num\" id=\"tNum\">--</div>"
    @"<div class=\"sub\" id=\"tSub\">&nbsp;</div></div>"
    @"<div class=\"right\"><div class=\"cap\">总计消耗</div>"
    @"<div class=\"num\" id=\"aNum\">--</div>"
    @"<div class=\"sub\" id=\"aSub\">&nbsp;</div></div>"
    @"</div>"
    // 工具分解
    @"<div class=\"section\"><h3>工具分解<span class=\"more\" id=\"toolMore\"></span></h3>"
    @"<div id=\"tools\"><div class=\"empty\">加载中…</div></div></div>"
    // 额度余量
    @"<div class=\"section\"><h3>额度余量</h3><div id=\"limits\"><div class=\"empty\">加载中…</div></div></div>"
    // 最近会话
    @"<div class=\"section\"><h3>最近会话</h3><div id=\"sessions\"><div class=\"empty\">加载中…</div></div></div>"
    // 7 天趋势
    @"<div class=\"section\"><h3>近 7 天趋势</h3><div class=\"trend\" id=\"trend\"></div></div>"
    @"</div>" // 滚动区域结束
    // 动作行
    @"<div class=\"actions\">"
    @"<button data-act=\"tokenusage\">仪表板</button>"
    @"<button data-act=\"space\">个人空间</button>"
    @"<button data-act=\"files\">文件</button>"
    @"</div></div>"
    @"<script>"
    @"var PANEL=null;"
    @"function fmtT(n){n=Number(n)||0;"
    @"if(n>=1e9)return(n/1e9).toFixed(2)+'B';"
    @"if(n>=1e6)return(n/1e6).toFixed(1)+'M';"
    @"if(n>=1e3)return(n/1e3).toFixed(1)+'K';return String(n);}"
    @"function fmtC(v){return '$'+(Number(v)||0).toFixed(2);}"
    @"function esc(s){return String(s==null?'':s).replace(/[&<>\"']/g,function(c){"
    @"return{'&':'&amp;','<':'&lt;','>':'&gt;','\"':'&quot;',\"'\":'&#39;'}[c];});}"
    // 周期切换：today(今日) / week(周) / month(月) / all(累计)
    @"function renderTotal(){if(!PANEL)return;var p=PANEL;"
    @"var num=document.getElementById('tNum'),sub=document.getElementById('tSub'),"
    @"cap=document.getElementById('tCap'),aNum=document.getElementById('aNum'),aSub=document.getElementById('aSub');"
    @"if(aNum&&p.allTime)aNum.textContent=fmtT(p.allTime.totalTokens);"
    @"if(aSub&&p.allTime)aSub.textContent=fmtC(p.allTime.costUsd);"
    @"var mode=(window._period||'today');"
    @"if(mode==='today'){cap.textContent='今日消耗';"
    @"num.textContent=fmtT(p.today&&p.today.totalTokens);"
    @"sub.textContent=fmtC(p.today&&p.today.costUsd);}"
    @"else if(mode==='week'){var t=0,c=0;"
    @"if(p.thisWeek&&p.thisWeek.totalTokens!=null){t=p.thisWeek.totalTokens;c=p.thisWeek.costUsd;}"
    @"else{(p.trends||[]).forEach(function(d){t+=Number(d.totalTokens)||0;c+=Number(d.costUsd)||0;});}"
    @"cap.textContent='本周消耗';num.textContent=fmtT(t);sub.textContent=fmtC(c);}"
    @"else if(mode==='month'){var t=0,c=0;"
    @"if(p.thisMonth&&p.thisMonth.totalTokens!=null){t=p.thisMonth.totalTokens;c=p.thisMonth.costUsd;}"
    @"else if(p.month&&p.month.totalTokens!=null){t=p.month.totalTokens;c=p.month.costUsd;}"
    @"cap.textContent='本月消耗';num.textContent=fmtT(t);sub.textContent=fmtC(c);}"
    @"else{cap.textContent='累计消耗';"
    @"num.textContent=fmtT(p.allTime&&p.allTime.totalTokens);"
    @"sub.textContent=fmtC(p.allTime&&p.allTime.costUsd);}}"
    @"function renderTools(){var el=document.getElementById('tools');"
    @"var arr=(PANEL&&PANEL.tools)||[];"
    @"if(!arr.length){el.innerHTML='<div class=\"empty\">暂无工具数据</div>';"
    @"document.getElementById('toolMore').textContent='';return;}"
    @"var max=Math.max.apply(null,arr.map(function(t){return Number(t.tokens)||0;}))||1;"
    @"var colors=['#73bdf5','#b394f4','#f4a073','#f1d973','#b7ead4'];var html='';"
    @"for(var i=0;i<arr.length;i++){var t=arr[i];var w=Math.max(2,Math.round((Number(t.tokens)||0)/max*100));"
    @"html+='<div class=\"tool-row\"><span class=\"dot\" style=\"background:'+colors[i%5]+';color:'+colors[i%5]+'\"></span>"
    @"<span class=\"name\">'+esc(t.name)+'</span>"
    @"<span class=\"bar\"><i style=\"width:'+w+'%;background:'+colors[i%5]+'\"></i></span>"
    @"<span class=\"val\">'+fmtT(t.tokens)+' · '+fmtC(t.costUsd)+'</span></div>';}"
    @"el.innerHTML=html;"
    @"document.getElementById('toolMore').textContent=arr.length+' 个工具';}"
    @"function renderLimits(items){var el=document.getElementById('limits');"
    @"if(!items||!items.length){el.innerHTML='<div class=\"empty\">暂无额度数据</div>';return;}"
    @"var html='';"
    @"for(var i=0;i<items.length&&i<5;i++){var it=items[i];"
    @"var cls=it.pct<20?'low':(it.pct<50?'mid':'');"
    @"html+='<div class=\"limit-row\"><span class=\"prov\">'+esc(it.provider)+'</span>"
    @"<span class=\"meter '+cls+'\"><i style=\"width:'+it.pct+'%\"></i></span>"
    @"<span class=\"pct\">'+it.pct.toFixed(0)+'%</span></div>';}"
    @"el.innerHTML=html;}"
    @"function renderSessions(){var el=document.getElementById('sessions');"
    @"var arr=(PANEL&&PANEL.sessions)||[];"
    @"if(!arr.length){el.innerHTML='<div class=\"empty\">暂无会话</div>';return;}"
    @"var html='';"
    @"for(var i=0;i<arr.length&&i<5;i++){var s=arr[i];"
    @"html+='<div class=\"sess-row\"><span class=\"name\">'+esc(s.id)+'</span>"
    @"<span class=\"meta\">'+fmtT(s.totalTokens)+' · '+fmtC(s.costUsd)+'</span></div>';}"
    @"el.innerHTML=html;}"
    @"function renderTrend(){var el=document.getElementById('trend');"
    @"var arr=(PANEL&&PANEL.trends)||[];arr=arr.slice(0,7).reverse();"
    @"if(!arr.length){el.innerHTML='<div class=\"empty\" style=\"padding:0\">暂无趋势数据</div>';return;}"
    @"var max=Math.max.apply(null,arr.map(function(d){return Number(d.totalTokens)||0;}))||1;"
    @"var today=(PANEL.today?String(PANEL.today._date||''):'');var html='';"
    @"for(var i=0;i<arr.length;i++){var d=arr[i];"
    @"var h=Math.max(4,Math.round((Number(d.totalTokens)||0)/max*100));"
    @"var lbl=(d.date||'').slice(5);"
    @"var isToday=(i===arr.length-1&&lbl===(PANEL._todayShort||''));"
    @"html+='<div class=\"col'+(isToday?' today':'')+'\"><i style=\"height:'+h+'%\"></i>"
    @"<span>'+esc(lbl)+'</span></div>';}"
    @"el.innerHTML=html;}"
    @"function renderAll(){renderTotal();renderTools();renderSessions();renderTrend();}"
    @"window.addEventListener('message',function(e){var d=e.data||{};"
    @"if(d.type==='panel'){PANEL=d.panel;renderAll();renderLimits(d.limits||[]);}"
    @"else if(d.type==='limits'){renderLimits(d.items||[]);}});"
    @"document.getElementById('tabs').addEventListener('click',function(e){"
    @"var b=e.target.closest('button');if(!b)return;"
    @"window._period=b.dataset.p;"
    @"document.querySelectorAll('#tabs button').forEach(function(x){"
    @"x.classList.toggle('active',x===b);});renderTotal();});"
    @"document.querySelectorAll('.actions button').forEach(function(b){"
    @"b.addEventListener('click',function(){"
    @"window.webkit.messageHandlers.native.postMessage({action:b.dataset.act});});});"
    // 超时兜底：若 1.2s 未收到数据，自动替换加载中为暂无数据
    @"setTimeout(function(){"
    @"if(!PANEL){"
    @"document.querySelectorAll('.empty').forEach(function(el){"
    @"if(el.textContent.indexOf('加载中')!==-1) el.textContent='暂无数据';"
    @"});}"
    @"}, 1200);"
    @"</script></body></html>";
}

- (NSPanel *)ensurePanel {
    if (self.popoverPanel) return self.popoverPanel;

    self.popoverPanel = [[PopoverPanel alloc] initWithContentRect:NSZeroRect
                                                   styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    self.popoverPanel.opaque = NO;
    self.popoverPanel.backgroundColor = [NSColor clearColor];
    self.popoverPanel.hasShadow = YES;
    self.popoverPanel.level = NSStatusWindowLevel;
    self.popoverPanel.hidesOnDeactivate = NO; // 失焦收起由 NSWindowDidResignKeyNotification 驱动，避免显示瞬间被误藏
    self.popoverPanel.releasedWhenClosed = NO;

    self.webView = [[WKWebView alloc] initWithFrame:NSMakeRect(0, 0, 360, 560)
                                      configuration:[self webViewConfig]];
    self.webView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    self.webView.navigationDelegate = self; // 页面加载完成后再注入数据，避免监听器未注册丢消息
    [self.webView setValue:@YES forKey:@"drawsBackground"]; // 透明背景
    self.popoverPanel.contentView = self.webView;

    NSNotificationCenter *nc = [NSNotificationCenter defaultCenter];
    [nc addObserver:self selector:@selector(panelClosed) name:NSWindowWillCloseNotification object:self.popoverPanel];
    [nc addObserver:self selector:@selector(panelResignedKey) name:NSWindowDidResignKeyNotification object:self.popoverPanel];
    return self.popoverPanel;
}

// 失焦自动收起（顶栏弹窗的标准交互），替代 hidesOnDeactivate
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
    // 关闭页面自身对 loopback 的网络访问需求：数据全部由原生注入
    return config;
}

- (void)togglePanel {
    if (self.popoverPanel && self.popoverPanel.isVisible) {
        [self.popoverPanel orderOut:nil];
        return;
    }

    NSPanel *panel = [self ensurePanel];

    // 定位：图标下方 8pt，水平居中于状态项，屏幕内钳制
    NSRect iconRect = [self.statusItem.button frame];
    NSWindow *buttonWindow = [self.statusItem.button window];
    if (buttonWindow) {
        iconRect = [self.statusItem.button convertRect:self.statusItem.button.bounds toView:nil];
        iconRect = [buttonWindow convertRectToScreen:iconRect];
    }
    NSScreen *screen = NSScreen.mainScreen;
    NSRect visible = screen.visibleFrame;
    CGFloat width = 360.0, height = 560.0;
    CGFloat x = iconRect.origin.x + iconRect.size.width / 2.0 - width / 2.0;
    x = MAX(visible.origin.x + 4, MIN(x, visible.origin.x + visible.size.width - width - 4));
    CGFloat y = iconRect.origin.y - height - 8;
    y = MAX(visible.origin.y + 4, y);
    [panel setFrame:NSMakeRect(x, y, width, height) display:YES];

    [self.webView loadHTMLString:[self panelHTML] baseURL:nil];
    [panel makeKeyAndOrderFront:nil];
    [panel makeFirstResponder:self.webView];
    // 数据注入延迟到 webView:didFinishNavigation:（页面监听器就绪后），避免首开丢消息
}

- (void)webView:(WKWebView *)webView didFinishNavigation:(WKNavigation *)navigation {
    [self fetchPanelData];
}

- (void)panelClosed {
    self.popoverPanel = nil;
    self.webView = nil;
}

#pragma mark - 本地 SQLite 离线/直读引擎

- (NSString *)locateTokenUsageDbPath {
    NSString *home = NSHomeDirectory();
    NSString *p1 = [home stringByAppendingPathComponent:@".natives-local/apps/tokenusage/data/tokenusage.db"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:p1]) return p1;
    NSString *p2 = [home stringByAppendingPathComponent:@".natives/apps/tokenusage/data/tokenusage.db"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:p2]) return p2;
    return nil;
}

- (NSString *)formatCompactTokens:(int64_t)tokens {
    if (tokens >= 1000000000LL) return [NSString stringWithFormat:@"%.2fB", (double)tokens / 1000000000.0];
    if (tokens >= 1000000LL) return [NSString stringWithFormat:@"%.1fM", (double)tokens / 1000000.0];
    if (tokens >= 1000LL) return [NSString stringWithFormat:@"%.1fK", (double)tokens / 1000.0];
    return [NSString stringWithFormat:@"%lld", tokens];
}

- (NSDictionary *)fetchStateFromLocalSqlite {
    NSString *dbPath = [self locateTokenUsageDbPath];
    if (!dbPath) return nil;

    sqlite3 *db = NULL;
    if (sqlite3_open_v2([dbPath UTF8String], &db, SQLITE_OPEN_READONLY, NULL) != SQLITE_OK) {
        if (db) sqlite3_close(db);
        return nil;
    }

    NSDateFormatter *df = [[NSDateFormatter alloc] init];
    [df setDateFormat:@"yyyy-MM-dd"];
    [df setTimeZone:[NSTimeZone localTimeZone]];
    NSDate *now = [NSDate date];
    NSString *todayStr = [df stringFromDate:now];

    NSDate *weekAgo = [now dateByAddingTimeInterval:-7 * 86400];
    NSString *weekStr = [df stringFromDate:weekAgo];

    NSString *monthStr = [todayStr substringToIndex:MIN((NSUInteger)7, todayStr.length)];
    monthStr = [monthStr stringByAppendingString:@"-01"];

    // 1) today
    int64_t todayTokens = 0, todayCostMicros = 0;
    sqlite3_stmt *stmt = NULL;
    const char *qToday = "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date = ?1";
    if (sqlite3_prepare_v2(db, qToday, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_text(stmt, 1, [todayStr UTF8String], -1, SQLITE_STATIC);
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            todayTokens = sqlite3_column_int64(stmt, 0);
            todayCostMicros = sqlite3_column_int64(stmt, 1);
        }
        sqlite3_finalize(stmt);
    }

    // 2) thisWeek
    int64_t weekTokens = 0, weekCostMicros = 0;
    const char *qWeek = "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1";
    if (sqlite3_prepare_v2(db, qWeek, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_text(stmt, 1, [weekStr UTF8String], -1, SQLITE_STATIC);
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            weekTokens = sqlite3_column_int64(stmt, 0);
            weekCostMicros = sqlite3_column_int64(stmt, 1);
        }
        sqlite3_finalize(stmt);
    }

    // 3) thisMonth
    int64_t monthTokens = 0, monthCostMicros = 0;
    const char *qMonth = "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates WHERE date >= ?1";
    if (sqlite3_prepare_v2(db, qMonth, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_text(stmt, 1, [monthStr UTF8String], -1, SQLITE_STATIC);
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            monthTokens = sqlite3_column_int64(stmt, 0);
            monthCostMicros = sqlite3_column_int64(stmt, 1);
        }
        sqlite3_finalize(stmt);
    }

    // 4) allTime
    int64_t allTokens = 0, allCostMicros = 0;
    const char *qAll = "SELECT COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost_micros), 0) FROM daily_aggregates";
    if (sqlite3_prepare_v2(db, qAll, -1, &stmt, NULL) == SQLITE_OK) {
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            allTokens = sqlite3_column_int64(stmt, 0);
            allCostMicros = sqlite3_column_int64(stmt, 1);
        }
        sqlite3_finalize(stmt);
    }

    // 5) tools Top 5
    NSMutableArray *tools = [NSMutableArray array];
    const char *qTools = "SELECT s.display_name, COALESCE(SUM(d.total_tokens), 0), COALESCE(SUM(d.cost_micros), 0) "
                         "FROM usage_sources s LEFT JOIN daily_aggregates d ON s.id = d.source_id "
                         "GROUP BY s.id ORDER BY SUM(d.total_tokens) DESC LIMIT 5";
    if (sqlite3_prepare_v2(db, qTools, -1, &stmt, NULL) == SQLITE_OK) {
        while (sqlite3_step(stmt) == SQLITE_ROW) {
            const unsigned char *nameChars = sqlite3_column_text(stmt, 0);
            NSString *name = nameChars ? [NSString stringWithUTF8String:(const char *)nameChars] : @"";
            int64_t t = sqlite3_column_int64(stmt, 1);
            int64_t c = sqlite3_column_int64(stmt, 2);
            [tools addObject:@{
                @"name": name,
                @"tokens": @(t),
                @"costUsd": @((double)c / 1000000.0)
            }];
        }
        sqlite3_finalize(stmt);
    }

    // 6) sessions Top 5
    NSMutableArray *sessions = [NSMutableArray array];
    const char *qSess = "SELECT session_id, source_id, "
                        "total_input_tokens + total_output_tokens + total_cache_read_tokens + total_cache_write_tokens + total_reasoning_tokens, "
                        "total_cost_micros, last_used_at "
                        "FROM sessions ORDER BY last_used_at DESC LIMIT 5";
    if (sqlite3_prepare_v2(db, qSess, -1, &stmt, NULL) == SQLITE_OK) {
        while (sqlite3_step(stmt) == SQLITE_ROW) {
            const unsigned char *sidChars = sqlite3_column_text(stmt, 0);
            const unsigned char *srcChars = sqlite3_column_text(stmt, 1);
            int64_t t = sqlite3_column_int64(stmt, 2);
            int64_t c = sqlite3_column_int64(stmt, 3);
            const unsigned char *actChars = sqlite3_column_text(stmt, 4);
            [sessions addObject:@{
                @"id": sidChars ? [NSString stringWithUTF8String:(const char *)sidChars] : @"",
                @"source": srcChars ? [NSString stringWithUTF8String:(const char *)srcChars] : @"",
                @"totalTokens": @(t),
                @"costUsd": @((double)c / 1000000.0),
                @"lastActive": actChars ? [NSString stringWithUTF8String:(const char *)actChars] : @""
            }];
        }
        sqlite3_finalize(stmt);
    }

    // 7) trends Top 7
    NSMutableArray *trends = [NSMutableArray array];
    const char *qTrends = "SELECT date, SUM(total_tokens), SUM(cost_micros) FROM daily_aggregates "
                          "GROUP BY date ORDER BY date DESC LIMIT 7";
    if (sqlite3_prepare_v2(db, qTrends, -1, &stmt, NULL) == SQLITE_OK) {
        while (sqlite3_step(stmt) == SQLITE_ROW) {
            const unsigned char *dateChars = sqlite3_column_text(stmt, 0);
            int64_t t = sqlite3_column_int64(stmt, 1);
            int64_t c = sqlite3_column_int64(stmt, 2);
            [trends addObject:@{
                @"date": dateChars ? [NSString stringWithUTF8String:(const char *)dateChars] : @"",
                @"totalTokens": @(t),
                @"costUsd": @((double)c / 1000000.0)
            }];
        }
        sqlite3_finalize(stmt);
    }

    // 8) worstLimit
    NSDictionary *worstLimit = nil;
    const char *qWorst = "SELECT provider_id, window_kind, remaining_percent, resets_at FROM limits_cache WHERE remaining_percent IS NOT NULL ORDER BY remaining_percent ASC LIMIT 1";
    if (sqlite3_prepare_v2(db, qWorst, -1, &stmt, NULL) == SQLITE_OK) {
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            const unsigned char *pChars = sqlite3_column_text(stmt, 0);
            const unsigned char *wChars = sqlite3_column_text(stmt, 1);
            double rem = sqlite3_column_double(stmt, 2);
            const unsigned char *rChars = sqlite3_column_text(stmt, 3);
            worstLimit = @{
                @"providerId": pChars ? [NSString stringWithUTF8String:(const char *)pChars] : @"",
                @"windowKind": wChars ? [NSString stringWithUTF8String:(const char *)wChars] : @"",
                @"remainingPercent": @(rem),
                @"resetsAt": rChars ? [NSString stringWithUTF8String:(const char *)rChars] : @""
            };
        }
        sqlite3_finalize(stmt);
    }

    // 8.1 若 limits_cache 尚无记录，从路由核心模块 proxy (model-host/state.json) 读取认证账号
    if (!worstLimit) {
        NSString *home = NSHomeDirectory();
        NSString *proxyPath = [home stringByAppendingPathComponent:@"Library/Application Support/Natives/model-host/state.json"];
        if ([[NSFileManager defaultManager] fileExistsAtPath:proxyPath]) {
            NSData *pData = [NSData dataWithContentsOfFile:proxyPath];
            if (pData) {
                NSDictionary *pJson = [NSJSONSerialization JSONObjectWithData:pData options:0 error:nil];
                if ([pJson isKindOfClass:[NSDictionary class]]) {
                    NSArray *accounts = pJson[@"accounts"];
                    if ([accounts isKindOfClass:[NSArray class]] && accounts.count > 0) {
                        NSDictionary *firstAcc = accounts[0];
                        NSString *p = firstAcc[@"provider"] ?: @"proxy";
                        worstLimit = @{
                            @"providerId": p,
                            @"windowKind": @"session",
                            @"remainingPercent": @(100.0),
                            @"resetsAt": @""
                        };
                    }
                }
            }
        }
    }

    // 9) 最近活跃日数据（若今日为 0，顶栏文案优先展示最近活跃日或累计）
    int64_t dispTokens = todayTokens;
    double dispCost = (double)todayCostMicros / 1000000.0;
    if (dispTokens == 0) {
        const char *qRecent = "SELECT SUM(total_tokens), SUM(cost_micros) FROM daily_aggregates GROUP BY date HAVING SUM(total_tokens) > 0 ORDER BY date DESC LIMIT 1";
        if (sqlite3_prepare_v2(db, qRecent, -1, &stmt, NULL) == SQLITE_OK) {
            if (sqlite3_step(stmt) == SQLITE_ROW) {
                dispTokens = sqlite3_column_int64(stmt, 0);
                dispCost = (double)sqlite3_column_int64(stmt, 1) / 1000000.0;
            }
            sqlite3_finalize(stmt);
        }
        if (dispTokens == 0 && allTokens > 0) {
            dispTokens = allTokens;
            dispCost = (double)allCostMicros / 1000000.0;
        }
    }

    sqlite3_close(db);

    NSString *tokensFormatted = [self formatCompactTokens:dispTokens];
    NSString *costFormatted = [NSString stringWithFormat:@"$%.2f", dispCost];
    NSString *displayText = [NSString stringWithFormat:@"%@ · %@", tokensFormatted, costFormatted];

    NSDictionary *panel = @{
        @"today": @{
            @"totalTokens": @(todayTokens),
            @"costUsd": @((double)todayCostMicros / 1000000.0)
        },
        @"thisWeek": @{
            @"totalTokens": @(weekTokens),
            @"costUsd": @((double)weekCostMicros / 1000000.0)
        },
        @"thisMonth": @{
            @"totalTokens": @(monthTokens),
            @"costUsd": @((double)monthCostMicros / 1000000.0)
        },
        @"allTime": @{
            @"totalTokens": @(allTokens),
            @"costUsd": @((double)allCostMicros / 1000000.0)
        },
        @"tools": tools,
        @"sessions": sessions,
        @"trends": trends
    };

    return @{
        @"mode": @"both",
        @"displayText": displayText,
        @"tooltip": [NSString stringWithFormat:@"Natives: %@ (%@)", displayText, todayTokens > 0 ? @"今日" : @"累计"],
        @"worstLimit": worstLimit ?: [NSNull null],
        @"panel": panel
    };
}

- (NSArray *)fetchLimitsFromLocalSqlite {
    NSMutableArray *items = [NSMutableArray array];

    // 1. 优先从 tokenusage.db 的 limits_cache 查询已同步的余量
    NSString *dbPath = [self locateTokenUsageDbPath];
    if (dbPath) {
        sqlite3 *db = NULL;
        if (sqlite3_open_v2([dbPath UTF8String], &db, SQLITE_OPEN_READONLY, NULL) == SQLITE_OK) {
            sqlite3_stmt *stmt = NULL;
            const char *q = "SELECT provider_id, window_kind, remaining_percent, label FROM limits_cache WHERE remaining_percent IS NOT NULL ORDER BY remaining_percent ASC LIMIT 5";
            if (sqlite3_prepare_v2(db, q, -1, &stmt, NULL) == SQLITE_OK) {
                while (sqlite3_step(stmt) == SQLITE_ROW) {
                    const unsigned char *pChars = sqlite3_column_text(stmt, 0);
                    const unsigned char *wChars = sqlite3_column_text(stmt, 1);
                    double rem = sqlite3_column_double(stmt, 2);
                    const unsigned char *lChars = sqlite3_column_text(stmt, 3);
                    double pct = MAX(0.0, MIN(100.0, rem));
                    NSString *prov = pChars ? [NSString stringWithUTF8String:(const char *)pChars] : @"--";
                    NSString *lbl = lChars ? [NSString stringWithUTF8String:(const char *)lChars] : prov;
                    [items addObject:@{
                        @"provider": lbl,
                        @"window": wChars ? [NSString stringWithUTF8String:(const char *)wChars] : @"",
                        @"pct": @(pct)
                    }];
                }
                sqlite3_finalize(stmt);
            }
            sqlite3_close(db);
        }
    }

    // 2. 接入路由核心模块 proxy (model-host / state.json) 读取认证账号与网关状态
    if (items.count == 0) {
        NSString *home = NSHomeDirectory();
        NSString *proxyPath = [home stringByAppendingPathComponent:@"Library/Application Support/Natives/model-host/state.json"];
        if ([[NSFileManager defaultManager] fileExistsAtPath:proxyPath]) {
            NSData *pData = [NSData dataWithContentsOfFile:proxyPath];
            if (pData) {
                NSDictionary *pJson = [NSJSONSerialization JSONObjectWithData:pData options:0 error:nil];
                if ([pJson isKindOfClass:[NSDictionary class]]) {
                    // 读取已认证的账号
                    NSArray *accounts = pJson[@"accounts"];
                    if ([accounts isKindOfClass:[NSArray class]]) {
                        for (NSDictionary *acc in accounts) {
                            if (![acc isKindOfClass:[NSDictionary class]]) continue;
                            NSString *prov = acc[@"provider"] ?: @"unknown";
                            NSString *label = acc[@"label"] ?: prov;
                            BOOL enabled = [acc[@"enabled"] boolValue];
                            if (enabled) {
                                [items addObject:@{
                                    @"provider": [NSString stringWithFormat:@"%@ (%@)", label, prov],
                                    @"window": @"session",
                                    @"pct": @(100.0)
                                }];
                            }
                        }
                    }

                    // 读取本地网关代理状态
                    NSDictionary *gw = pJson[@"gateway"];
                    if ([gw isKindOfClass:[NSDictionary class]]) {
                        NSString *gwState = gw[@"state"] ?: @"";
                        if ([gwState isEqualToString:@"running"]) {
                            NSNumber *port = gw[@"port"] ?: @(52567);
                            [items addObject:@{
                                @"provider": [NSString stringWithFormat:@"Local Gateway (:%@)", port],
                                @"window": @"proxy",
                                @"pct": @(100.0)
                            }];
                        }
                    }
                }
            }
        }
    }

    return items;
}

- (void)loadLocalDataToPanel {
    if (!self.webView) return;
    NSDictionary *state = [self fetchStateFromLocalSqlite];
    if (state) {
        NSString *disp = state[@"displayText"];
        if (disp) {
            NSData *d = [NSJSONSerialization dataWithJSONObject:state options:0 error:nil];
            if (d) {
                [self updateWithStateJson:[[NSString alloc] initWithData:d encoding:NSUTF8StringEncoding]];
            }
        }
        id panel = state[@"panel"];
        if (panel) {
            [self postToPanel:@{@"type": @"panel", @"panel": panel}];
        }
        NSArray *localLimits = [self fetchLimitsFromLocalSqlite];
        [self postToPanel:@{@"type": @"limits", @"items": localLimits ?: @[]}];
    } else {
        // 本地完全无数据库或空数据：发送空面板数据，消除“加载中…”
        NSDictionary *emptyPanel = @{
            @"today": @{@"totalTokens": @0, @"costUsd": @0.0},
            @"thisWeek": @{@"totalTokens": @0, @"costUsd": @0.0},
            @"thisMonth": @{@"totalTokens": @0, @"costUsd": @0.0},
            @"allTime": @{@"totalTokens": @0, @"costUsd": @0.0},
            @"tools": @[],
            @"sessions": @[],
            @"trends": @[]
        };
        [self postToPanel:@{@"type": @"panel", @"panel": emptyPanel}];
        [self postToPanel:@{@"type": @"limits", @"items": @[]}];
    }
}

// 拉取 /api/tray/state（含 panel 聚合：总览/工具/会话/趋势）与 /api/limits，
// 若网络不可达则无缝降级至本地 SQLite 直读引擎。
- (void)fetchPanelData {
    if (!self.webView) return;

    __block BOOL panelLoaded = NO;
    __block BOOL limitsLoaded = NO;

    NSURLSessionConfiguration *config = [NSURLSessionConfiguration ephemeralSessionConfiguration];
    config.timeoutIntervalForRequest = 1.5;
    NSURLSession *session = [NSURLSession sessionWithConfiguration:config];

    // 1) tray/state：panel 聚合 + 周期摘要
    NSURL *stateURL = [NSURL URLWithString:[NSString stringWithFormat:@"%@/api/tray/state", self.runtimeBaseUrl]];
    [[session dataTaskWithURL:stateURL completionHandler:^(NSData *data, NSURLResponse *resp, NSError *error) {
        if (!error && data) {
            NSDictionary *json = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
            if ([json isKindOfClass:[NSDictionary class]]) {
                dispatch_async(dispatch_get_main_queue(), ^{
                    NSString *text = [json[@"displayText"] isKindOfClass:[NSString class]] ? json[@"displayText"] : nil;
                    if (text) [self updateWithStateJson:data ? [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] : nil];
                });

                id panel = json[@"panel"];
                if ([panel isKindOfClass:[NSDictionary class]]) {
                    panelLoaded = YES;
                    [self postToPanel:@{@"type": @"panel", @"panel": panel}];
                }
            }
        }
        if (!panelLoaded) {
            dispatch_async(dispatch_get_main_queue(), ^{
                [self loadLocalDataToPanel];
            });
        }
    }] resume];

    // 2) limits：额度余量列表
    NSURL *limitsURL = [NSURL URLWithString:[NSString stringWithFormat:@"%@/api/limits", self.runtimeBaseUrl]];
    [[session dataTaskWithURL:limitsURL completionHandler:^(NSData *data, NSURLResponse *resp, NSError *error) {
        if (!error && data) {
            NSDictionary *json = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
            if ([json isKindOfClass:[NSDictionary class]]) {
                NSArray *limits = json[@"limits"];
                if ([limits isKindOfClass:[NSArray class]]) {
                    NSMutableArray *items = [NSMutableArray array];
                    for (NSDictionary *row in limits) {
                        if (![row isKindOfClass:[NSDictionary class]]) continue;
                        id rem = row[@"remainingPercent"];
                        if (![rem isKindOfClass:[NSNumber class]]) continue;
                        double pct = MAX(0.0, MIN(100.0, [rem doubleValue]));
                        NSString *prov = [row[@"providerId"] isKindOfClass:[NSString class]] ? row[@"providerId"] : @"--";
                        NSString *window = [row[@"windowKind"] isKindOfClass:[NSString class]] ? row[@"windowKind"] : @"";
                        [items addObject:@{@"provider": prov, @"pct": @(pct), @"window": window}];
                    }
                    // 按余量升序，最紧张的在最上面
                    [items sortUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
                        return [@([a[@"pct"] doubleValue]) compare:@([b[@"pct"] doubleValue])];
                    }];

                    limitsLoaded = YES;
                    dispatch_async(dispatch_get_main_queue(), ^{
                        if (!self.webView) return;
                        [self postToPanel:@{@"type": @"limits", @"items": items}];
                    });
                }
            }
        }
        if (!limitsLoaded) {
            dispatch_async(dispatch_get_main_queue(), ^{
                NSArray *localLimits = [self fetchLimitsFromLocalSqlite];
                if (!self.webView) return;
                [self postToPanel:@{@"type": @"limits", @"items": localLimits ?: @[]}];
            });
        }
    }] resume];

    [session finishTasksAndInvalidate];
}

- (void)postToPanel:(NSDictionary *)msg {
    if (!self.webView) return;
    NSData *data = [NSJSONSerialization dataWithJSONObject:msg options:0 error:nil];
    if (!data) return;
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    [self.webView evaluateJavaScript:[NSString stringWithFormat:@"window.postMessage(%@, '*')", json] completionHandler:nil];
}

- (void)userContentController:(WKUserContentController *)userContentController
      didReceiveScriptMessage:(WKScriptMessage *)message {
    if (![message.body isKindOfClass:[NSDictionary class]]) return;
    NSString *action = message.body[@"action"];
    if ([action isKindOfClass:[NSString class]]) {
        [self openTargetApp:action];
        [self.popoverPanel orderOut:nil];
    }
}

#pragma mark - NSMenuDelegate

- (void)menuDidClose:(NSMenu *)menu {
    (void)menu; // NSMenu 仅保留给排版子菜单使用（数据卡已由面板承载）
}

- (void)actTogglePanel:(id)sender {
    (void)sender;
    [self togglePanel];
}

- (void)buildMenu {
    self.statusMenu = [[NSMenu alloc] initWithTitle:@"Token Monitor"];
    self.statusMenu.delegate = self;

    // Token Monitor 风格头部数据卡（view-based，玻璃底 + 等宽数字 + 进度条）
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

    // 动作入口
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

    // 刷新
    NSMenuItem *refreshItem = [[NSMenuItem alloc] initWithTitle:@"立即刷新统计"
                                                         action:@selector(fetchAndUpdateState)
                                                  keyEquivalent:@"r"];
    refreshItem.target = self;
    [self.statusMenu addItem:refreshItem];

    // 排版子菜单
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

    // 退出
    NSMenuItem *quitItem = [[NSMenuItem alloc] initWithTitle:@"退出"
                                                      action:@selector(actQuit)
                                               keyEquivalent:@"q"];
    quitItem.target = self;
    [self.statusMenu addItem:quitItem];

    // 注意：不再把菜单挂到 statusItem.menu —— 点击图标由 togglePanel 弹出自定义面板；
    // statusMenu 保留给右键长按等系统行为（未挂载即不弹出）。
}

- (void)actChangeDisplayMode:(NSMenuItem *)sender {
    self.displayMode = sender.tag;
    [self refreshButtonUI];
    [self buildMenu];
}

- (void)refreshButtonUI {
    NSStatusBarButton *button = self.statusItem.button;
    if (!button) return;

    NSString *title = @"";
    switch (self.displayMode) {
        case NativesStatusBarDisplayModeBoth:
            title = [NSString stringWithFormat:@" %@", self.latestDisplayText];
            break;
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
    }

    button.title = title;
    button.toolTip = self.latestTooltip;
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
            // HTTP 离线 / 运行时未启动：回退至本地 SQLite 只读数据
            NSDictionary *localState = [self fetchStateFromLocalSqlite];
            if (localState) {
                NSData *localData = [NSJSONSerialization dataWithJSONObject:localState options:0 error:nil];
                if (localData) {
                    NSString *localText = [[NSString alloc] initWithData:localData encoding:NSUTF8StringEncoding];
                    [self updateWithStateJson:localText];
                }
            }
        }
    }];
    [task resume];
    [session finishTasksAndInvalidate];
}

- (NSString *)resolvedExtensionId {
    // 1. 优先读取环境变量
    char *envId = getenv("NATIVES_EXTENSION_ID");
    if (envId && strlen(envId) == 32) {
        return [NSString stringWithUTF8String:envId];
    }

    // 2. 读取系统应用支持目录中的 extension-id 配置
    NSString *home = NSHomeDirectory();
    NSArray *paths = @[
        [home stringByAppendingPathComponent:@"Library/Application Support/Natives/extension-id"],
        [home stringByAppendingPathComponent:@"Library/Application Support/Natives-Local/extension-id"]
    ];
    for (NSString *p in paths) {
        if ([[NSFileManager defaultManager] fileExistsAtPath:p]) {
            NSString *content = [NSString stringWithContentsOfFile:p encoding:NSUTF8StringEncoding error:nil];
            if (content) {
                NSString *trimmed = [content stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
                if (trimmed.length == 32) {
                    return trimmed;
                }
            }
        }
    }

    // 3. 宏定义或稳定候选 ID
#ifdef EXTENSION_ID
    return @EXTENSION_ID;
#else
    return @"gehmgcnlpdepnpmcbbdaijabcjdnbfmh";
#endif
}

- (void)openInBrowser:(NSString *)url {
    NSMutableArray *browsers = [NSMutableArray array];
    char *preferred = getenv("NATIVES_BROWSER");
    if (preferred && strlen(preferred) > 0) {
        [browsers addObject:[NSString stringWithUTF8String:preferred]];
    }
    [browsers addObject:@"Google Chrome"];
    [browsers addObject:@"Chromium"];

    for (NSString *b in browsers) {
        NSTask *task = [[NSTask alloc] init];
        task.launchPath = @"/usr/bin/open";
        task.arguments = @[@"-a", b, url];
        NSError *err = nil;
        if ([task launchAndReturnError:&err]) {
            [task waitUntilExit];
            if (task.terminationStatus == 0) return;
        }
    }

    // 兜底：直接由系统默认浏览器打开 URL
    [[NSWorkspace sharedWorkspace] openURL:[NSURL URLWithString:url]];
}

- (void)openTargetApp:(NSString *)appId {
    NSString *extId = [self resolvedExtensionId];
    NSString *page = @"app.html?app=tokenusage";
    if ([appId isEqualToString:@"space"]) {
        page = @"space.html";
    } else if ([appId isEqualToString:@"files"]) {
        page = @"files.html";
    }

    NSString *url = [NSString stringWithFormat:@"chrome-extension://%@/%@", extId, page];
    [self openInBrowser:url];
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
