// TokenMonitorHeaderView.h: macOS 状态栏下拉菜单的 Header 视图与强调色额度条
#import <AppKit/AppKit.h>

#pragma mark - 强调色进度条

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
