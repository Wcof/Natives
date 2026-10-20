// NativesStatusBar.h: macOS 顶部系统菜单栏（NSStatusItem）常驻“标签栏”组件
// 负责系统顶栏图标、指标排版（Token/费用/额度条）与交互呼出。

#import <AppKit/AppKit.h>

typedef NS_ENUM(NSInteger, NativesStatusBarDisplayMode) {
    NativesStatusBarDisplayModeBoth = 0,    // 1.25M · $3.42
    NativesStatusBarDisplayModeTokens = 1,  // 1.25M
    NativesStatusBarDisplayModeCost = 2,    // $3.42
    NativesStatusBarDisplayModeBars = 3,    // 额度进度条模式
    NativesStatusBarDisplayModeIconOnly = 4 // 仅图标
};

static inline BOOL NativesStatusBarUsesWeb(void) {
    NSString *entry = NSProcessInfo.processInfo.environment[@"NATIVES_ENTRY"];
    if (!entry) entry = [NSUserDefaults.standardUserDefaults stringForKey:@"NativesPreferredEntry"];
    return [entry isEqualToString:@"web"];
}

static inline NSURL *NativesStatusBarTargetURL(NSString *target) {
    NSDictionary *routes = @{@"space": @"space.html", @"files": @"files.html", @"tokenusage": @"app.html?app=tokenusage"};
    NSString *route = [target isKindOfClass:NSString.class] ? routes[target] : nil;
    if (!route) return nil;

    if (NativesStatusBarUsesWeb()) {
        NSString *port = [[[NSProcessInfo processInfo] environment] objectForKey:@"NATIVES_WEB_PORT"] ?: @"8770";
        if ([port rangeOfCharacterFromSet:NSCharacterSet.decimalDigitCharacterSet.invertedSet].location != NSNotFound || port.integerValue < 1 || port.integerValue > 65535) return nil;
        return [NSURL URLWithString:[NSString stringWithFormat:@"http://127.0.0.1:%@/%@", port, route]];
    }

#ifdef EXTENSION_ID
    NSString *extensionId = [NSString stringWithUTF8String:EXTENSION_ID];
#else
    NSString *extensionId = @"gehmgcnlpdepnpmcbbdaijabcjdnbfmh";
#endif
    return [NSURL URLWithString:[NSString stringWithFormat:@"chrome-extension://%@/%@", extensionId, route]];
}

@interface NativesStatusBar : NSObject

@property (nonatomic, strong) NSStatusItem *statusItem;
@property (nonatomic, assign) NativesStatusBarDisplayMode displayMode;
@property (nonatomic, copy) NSString *runtimeBaseUrl;
@property (nonatomic, strong) id clickOutsideMonitor;

+ (instancetype)sharedBar;
- (void)setupStatusItem;
- (void)updateWithStateJson:(NSString *)jsonText;
- (void)fetchAndUpdateState;
- (void)openTargetApp:(NSString *)appId;
- (void)closePanel;

// 清理同二进制的其它实例（SIGTERM → 宽限 → SIGKILL）。
// 仅启动时清理旧实例；退出不得清扫后来启动的实例。
void NativesStatusBarSweepProjectProcesses(void);

@end
