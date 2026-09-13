/* Natives 主入口包装（实施方案 §1.1/§1.3/§1.4 + 用户体验深度重构）：
 * 1. 扩展已安装就绪状态：
 *    展示「Natives 控制中心」窗口，提供三个核心功能：
 *    - 【访问浏览器】：一键在 Chrome 打开 Natives 空间
 *    - 【重装扩展】：重新打开引导与下载文件夹，触发重新检测
 *    - 【卸载 Natives】：二次确认，管理员权限执行 uninstall.sh，保留个人数据
 * 2. 首次未装载状态：
 *    展示「装载引导」窗口，支持一键打开 Chrome 扩展管理页、定位下载文件夹、复制路径与重新检测。
 * 3. 注册 natives-setup[-local]:// scheme，支持引导页直接唤起操作。 */
#import <AppKit/AppKit.h>
#include <spawn.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

#ifndef SOURCE_ROOT
#define SOURCE_ROOT "/Library/Application Support/Natives-Local"
#endif
#ifndef EXTENSION_ID
#define EXTENSION_ID "gehmgcnlpdepnpmcbbdaijabcjdnbfmh"
#endif
#ifndef SETUP_SCHEME
#define SETUP_SCHEME "natives-setup-local"
#endif
#define ONBOARDING_PATH @"/Applications/Natives.app/Contents/Resources/onboarding/index.html"
#define TOTAL_DEADLINE_SECONDS 600

static NSTask *activeTask = nil;
static NSTimeInterval totalWaited = 0;
static NSTextField *statusField = nil;
static NSWindow *currentWindow = nil;

static int run_host_sync(NSArray<NSString *> *args) {
    NSTask *task = [[NSTask alloc] init];
    task.launchPath = [NSString stringWithFormat:@"%s/native-file-host", SOURCE_ROOT];
    task.arguments = args;
    task.standardOutput = [NSPipe pipe];
    task.standardError = [NSPipe pipe];
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) return 127;
    [task waitUntilExit];
    return task.terminationStatus;
}

static BOOL chromeInstalled(void) {
    return access("/Applications/Google Chrome.app", F_OK) == 0
        || access([NSHomeDirectory() stringByAppendingPathComponent:@"Applications/Google Chrome.app"].fileSystemRepresentation, F_OK) == 0;
}

static void openInChrome(NSString *target) {
    NSTask *task = [[NSTask alloc] init];
    task.launchPath = @"/usr/bin/open";
    task.arguments = @[@"-a", @"Google Chrome", target];
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) NSLog(@"open failed: %@", error);
}

static void revealFolder(void) {
    run_host_sync(@[@"--reveal-extension-dir"]);
}

static void copyPath(void) {
    NSString *dir = [NSString stringWithFormat:@"%s/ChromeExtension", SOURCE_ROOT];
    [[NSPasteboard generalPasteboard] clearContents];
    [[NSPasteboard generalPasteboard] setString:dir forType:NSPasteboardTypeString];
}

static void startDetection(void);
static void showSetupWindow(void);
static void showReadyWindow(void);

static void openProduct(void) {
    openInChrome([NSString stringWithFormat:@"chrome-extension://%s/space.html", EXTENSION_ID]);
}

static void executeUninstall(void) {
    NSAlert *confirm = [[NSAlert alloc] init];
    confirm.alertStyle = NSAlertStyleCritical;
    confirm.messageText = @"确定要卸载 Natives 吗？";
    confirm.informativeText = @"此操作将彻底清除系统后台服务、浏览器 Native 注册及主应用程序。\n\n您的个人数据、扩展配置与基金记账 (~/.natives) 将被完整保留。";
    [confirm addButtonWithTitle:@"确定卸载"];
    [confirm addButtonWithTitle:@"取消"];
    if ([confirm runModal] != NSAlertFirstButtonReturn) return;

    NSString *script = [NSString stringWithFormat:@"\"%s/uninstall.sh\"", SOURCE_ROOT];
    NSString *appleScript = [NSString stringWithFormat:@"do shell script \"%@\" with administrator privileges", script];
    NSTask *task = [[NSTask alloc] init];
    task.launchPath = @"/usr/bin/osascript";
    task.arguments = @[@"-e", appleScript];
    NSError *error = nil;
    if ([task launchAndReturnError:&error]) {
        [task waitUntilExit];
        if (task.terminationStatus == 0) {
            NSAlert *done = [[NSAlert alloc] init];
            done.alertStyle = NSAlertStyleInformational;
            done.messageText = @"卸载完成";
            done.informativeText = @"Natives 已成功从系统中移除。如需彻底删除个人数据，可手动删除 ~/.natives 目录。";
            [done addButtonWithTitle:@"好"];
            [done runModal];
            [NSApp terminate:nil];
            return;
        }
    }
    NSAlert *failed = [[NSAlert alloc] init];
    failed.alertStyle = NSAlertStyleCritical;
    failed.messageText = @"卸载未完成";
    failed.informativeText = @"管理员授权被取消或卸载脚本执行失败。";
    [failed addButtonWithTitle:@"好"];
    [failed runModal];
}

static void startDetection(void) {
    if (activeTask && activeTask.isRunning) return;
    NSTask *task = [[NSTask alloc] init];
    task.launchPath = [NSString stringWithFormat:@"%s/native-file-host", SOURCE_ROOT];
    task.arguments = @[@"--launcher-setup", @"--setup-timeout-secs", @"60", @"--no-open"];
    task.standardOutput = [NSPipe pipe];
    task.standardError = [NSPipe pipe];
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) return;
    activeTask = task;
}

@interface AppDelegate : NSObject <NSApplicationDelegate, NSWindowDelegate>
@end
@implementation AppDelegate
- (void)actOpenBrowser:(id)sender {
    openProduct();
    [NSApp terminate:nil];
}
- (void)actReinstall:(id)sender {
    if (currentWindow) [currentWindow close];
    showSetupWindow();
    openInChrome(ONBOARDING_PATH);
    openInChrome(@"chrome://extensions");
    revealFolder();
    startDetection();
}
- (void)actUninstall:(id)sender {
    executeUninstall();
}
- (void)actDetect:(id)sender { startDetection(); }
- (void)actExtensions:(id)sender { openInChrome(@"chrome://extensions"); }
- (void)actFolder:(id)sender { revealFolder(); }
- (void)actCopy:(id)sender { copyPath(); }

- (void)handleAction:(NSString *)action {
    NSString *prefix = [NSString stringWithFormat:@"%s://", SETUP_SCHEME];
    if (![action hasPrefix:prefix]) return;
    NSString *act = [action substringFromIndex:prefix.length];
    if ([act isEqualToString:@"extensions"]) openInChrome(@"chrome://extensions");
    else if ([act isEqualToString:@"folder"]) revealFolder();
    else if ([act isEqualToString:@"copy"]) copyPath();
    else if ([act isEqualToString:@"detect"]) startDetection();
    else if ([act isEqualToString:@"open"]) { openProduct(); [NSApp terminate:nil]; }
}
- (void)application:(NSApplication *)app openURLs:(NSArray<NSURL *> *)urls {
    for (NSURL *url in urls) [self handleAction:url.absoluteString];
}
- (void)windowWillClose:(NSNotification *)note {
    if (activeTask.isRunning) [activeTask terminate];
    [NSApp terminate:nil];
}
@end

static AppDelegate *delegate = nil;

// 1. 扩展已就绪控制中心窗口 (Ready Dashboard)
static void showReadyWindow(void) {
    NSWindow *win = [[NSWindow alloc]
        initWithContentRect:NSMakeRect(0, 0, 460, 210)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable
        backing:NSBackingStoreBuffered defer:NO];
    win.title = @"Natives 控制中心";
    [win center];
    win.delegate = delegate;

    // Header 图标 + 标题 + 状态
    NSImageView *iconView = [[NSImageView alloc] initWithFrame:NSMakeRect(24, 130, 56, 56)];
    NSString *iconPath = [[NSBundle mainBundle] pathForResource:@"natives" ofType:@"icns"];
    if (!iconPath) iconPath = [NSString stringWithFormat:@"%s/resources/natives.icns", SOURCE_ROOT];
    NSImage *appIcon = [[NSImage alloc] initWithContentsOfFile:iconPath];
    if (!appIcon) appIcon = [NSImage imageNamed:NSImageNameApplicationIcon];
    iconView.image = appIcon;
    [win.contentView addSubview:iconView];

    NSTextField *title = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 156, 340, 26)];
    title.editable = NO; title.bezeled = NO; title.drawsBackground = NO;
    title.font = [NSFont boldSystemFontOfSize:18];
    title.stringValue = @"Natives";
    [win.contentView addSubview:title];

    NSTextField *status = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 134, 340, 20)];
    status.editable = NO; status.bezeled = NO; status.drawsBackground = NO;
    status.font = [NSFont systemFontOfSize:13];
    status.textColor = [NSColor colorWithCalibratedRed:0.18 green:0.65 blue:0.34 alpha:1.0];
    status.stringValue = @"● 运行就绪 · 扩展已连接";
    [win.contentView addSubview:status];

    // 分割线
    NSBox *sep = [[NSBox alloc] initWithFrame:NSMakeRect(24, 115, 412, 1)];
    sep.boxType = NSBoxSeparator;
    [win.contentView addSubview:sep];

    // 三个核心操作按钮
    // 1. 【访问浏览器】(Primary)
    NSButton *btnOpen = [[NSButton alloc] initWithFrame:NSMakeRect(24, 66, 412, 36)];
    btnOpen.title = @"访问浏览器 (进入 Natives)";
    btnOpen.bezelStyle = NSBezelStyleRounded;
    btnOpen.keyEquivalent = @"\r"; // 回车触发
    btnOpen.target = delegate;
    btnOpen.action = @selector(actOpenBrowser:);
    [win.contentView addSubview:btnOpen];

    // 2. 【重装扩展】
    NSButton *btnReinstall = [[NSButton alloc] initWithFrame:NSMakeRect(24, 22, 198, 32)];
    btnReinstall.title = @"重装扩展";
    btnReinstall.bezelStyle = NSBezelStyleRounded;
    btnReinstall.target = delegate;
    btnReinstall.action = @selector(actReinstall:);
    [win.contentView addSubview:btnReinstall];

    // 3. 【卸载 Natives】
    NSButton *btnUninstall = [[NSButton alloc] initWithFrame:NSMakeRect(238, 22, 198, 32)];
    btnUninstall.title = @"卸载 Natives";
    btnUninstall.bezelStyle = NSBezelStyleRounded;
    btnUninstall.target = delegate;
    btnUninstall.action = @selector(actUninstall:);
    [win.contentView addSubview:btnUninstall];

    currentWindow = win;
    [win makeKeyAndOrderFront:nil];
}

// 2. 首次安装引导窗口 (Setup Guided Window)
static void showSetupWindow(void) {
    NSWindow *win = [[NSWindow alloc]
        initWithContentRect:NSMakeRect(0, 0, 480, 230)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
        backing:NSBackingStoreBuffered defer:NO];
    win.title = @"Natives 扩展装载引导";
    [win center];
    win.delegate = delegate;

    NSTextField *status = [[NSTextField alloc] initWithFrame:NSMakeRect(24, 150, 432, 60)];
    status.editable = NO; status.bezeled = NO; status.drawsBackground = NO;
    status.font = [NSFont systemFontOfSize:13];
    status.stringValue = @"首次使用，需要将随包扩展加载至 Chrome 浏览器。\n请开启「开发者模式」并将已定位的文件夹拖入 Chrome 页面。";
    statusField = status;
    [win.contentView addSubview:status];

    NSButton *b1 = [[NSButton alloc] initWithFrame:NSMakeRect(24, 108, 208, 32)];
    b1.title = @"打开扩展管理页";
    b1.bezelStyle = NSBezelStyleRounded;
    b1.target = delegate; b1.action = @selector(actExtensions:);
    [win.contentView addSubview:b1];

    NSButton *b2 = [[NSButton alloc] initWithFrame:NSMakeRect(248, 108, 208, 32)];
    b2.title = @"显示扩展文件夹";
    b2.bezelStyle = NSBezelStyleRounded;
    b2.target = delegate; b2.action = @selector(actFolder:);
    [win.contentView addSubview:b2];

    NSButton *b3 = [[NSButton alloc] initWithFrame:NSMakeRect(24, 68, 208, 32)];
    b3.title = @"复制目录路径";
    b3.bezelStyle = NSBezelStyleRounded;
    b3.target = delegate; b3.action = @selector(actCopy:);
    [win.contentView addSubview:b3];

    NSButton *b4 = [[NSButton alloc] initWithFrame:NSMakeRect(248, 68, 208, 32)];
    b4.title = @"重新检测并进入";
    b4.bezelStyle = NSBezelStyleRounded;
    b4.keyEquivalent = @"\r";
    b4.target = delegate; b4.action = @selector(actDetect:);
    [win.contentView addSubview:b4];

    // 取消退出
    NSButton *bCancel = [[NSButton alloc] initWithFrame:NSMakeRect(24, 20, 432, 30)];
    bCancel.title = @"取消并稍后完成";
    bCancel.bezelStyle = NSBezelStyleRounded;
    bCancel.target = NSApp; bCancel.action = @selector(terminate:);
    [win.contentView addSubview:bCancel];

    // 定时轮询检测连接
    [NSTimer scheduledTimerWithTimeInterval:0.5 repeats:YES block:^(NSTimer *timer) {
        if (!currentWindow || !currentWindow.isVisible) { [timer invalidate]; return; }
        if (activeTask && !activeTask.isRunning && activeTask.terminationStatus == 0) {
            openProduct();
            [timer invalidate];
            [NSApp terminate:nil];
            return;
        }
        totalWaited += 0.5;
        if (totalWaited >= TOTAL_DEADLINE_SECONDS && activeTask.isRunning) {
            [activeTask terminate];
            statusField.stringValue = @"已等待超过 10 分钟。装载完成后请点击「重新检测并进入」。";
        }
    }];

    currentWindow = win;
    [win makeKeyAndOrderFront:nil];
}

int main(int argc, char **argv) {
    (void)argc; (void)argv;
    NSApplication *app = [NSApplication sharedApplication];
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    delegate = [AppDelegate new];
    app.delegate = delegate;
    [app activateIgnoringOtherApps:YES];

    NSString *manifestPath = [NSString stringWithFormat:@"%s/ChromeExtension/manifest.json", SOURCE_ROOT];
    BOOL installed = [[NSFileManager defaultManager] fileExistsAtPath:manifestPath];

    if (chromeInstalled() && installed) {
        int code = run_host_sync(@[@"--launcher-default", @"--no-open"]);
        if (code == 0) {
            // 扩展已装载就绪：展示控制中心（访问浏览器 / 重装 / 卸载）！
            showReadyWindow();
            [app run];
            return 0;
        }
    }

    // 首次引导流程
    if (!installed) {
        NSAlert *alert = [[NSAlert alloc] init];
        alert.alertStyle = NSAlertStyleCritical;
        alert.messageText = @"Natives";
        alert.informativeText = @"安装不完整：缺少随包扩展目录，请重新安装 Natives。";
        [alert addButtonWithTitle:@"好"];
        [alert runModal];
        return 1;
    }

    openInChrome(ONBOARDING_PATH);
    openInChrome(@"chrome://extensions");
    revealFolder();
    startDetection();
    showSetupWindow();
    [app run];
    return 0;
}
