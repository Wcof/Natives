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

@interface NativesStatusBar : NSObject

@property (nonatomic, strong) NSStatusItem *statusItem;
@property (nonatomic, assign) NativesStatusBarDisplayMode displayMode;
@property (nonatomic, copy) NSString *runtimeBaseUrl;

+ (instancetype)sharedBar;
- (void)setupStatusItem;
- (void)updateWithStateJson:(NSString *)jsonText;
- (void)fetchAndUpdateState;
- (void)openTargetApp:(NSString *)appId;

// 清理同二进制的其它实例（SIGTERM → 宽限 → SIGKILL）。
// 启动时调用 = 干掉旧实例再拉起；退出时调用（atexit）= 兜底防僵尸。
void NativesStatusBarSweepProjectProcesses(void);

@end
