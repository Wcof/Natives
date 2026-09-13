/* Natives 主入口（§1.1/§1.3/§1.4 + 用户要求 2026-09-13：引导页按钮直达）。
 * 注册 natives-setup[-local]:// scheme：引导页按钮点击后由系统唤起本应用
 * 执行固定动作（扩展管理页/显示文件夹/复制路径/重新检测）——白名单枚举、
 * 无参数、无网络；未识别动作忽略。状态窗非模态、始终可取消、10 分钟截止。 */
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
static NSWindow *statusWindow = nil;

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

// Chrome 目标显式走 Chrome；扩展目录动作经 Host 单一来源（--reveal-extension-dir）。
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

static void updateStatus(NSString *text) {
    dispatch_async(dispatch_get_main_queue(), ^{
        [statusField setStringValue:text];
    });
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

static void openProductIfReady(void) {
    // 成功交接：打开产品页并退出 Launcher（图标保留）。
    [[NSWorkspace sharedWorkspace] openURL:
        [NSURL URLWithString:[NSString stringWithFormat:
            @"chrome-extension://%s/space.html", EXTENSION_ID]]];
    [NSApp terminate:nil];
}

@interface AppDelegate : NSObject <NSApplicationDelegate, NSWindowDelegate>
@property(strong) NSWindow *window;
@end
@implementation AppDelegate
- (void)openExtensionsPage { openInChrome(@"chrome://extensions"); }
- (void)handleAction:(NSString *)action {
    NSString *prefix = [NSString stringWithFormat:@"%s://", SETUP_SCHEME];
    if (![action hasPrefix:prefix]) return; // 白名单 scheme，忽略其他来源
    NSString *act = [action substringFromIndex:prefix.length];
    if ([act isEqualToString:@"extensions"]) [self openExtensionsPage];
    else if ([act isEqualToString:@"folder"]) revealFolder();
    else if ([act isEqualToString:@"copy"]) copyPath();
    else if ([act isEqualToString:@"detect"]) { startDetection(); }
    // 未识别动作忽略：HTML 不接收任意命令（§1.3）。
}
- (void)application:(NSApplication *)app openURLs:(NSArray<NSURL *> *)urls {
    for (NSURL *url in urls) [self handleAction:url.absoluteString];
}
- (void)windowWillClose:(NSNotification *)note {
    if (activeTask.isRunning) [activeTask terminate];
}
- (void)actDetect:(id)sender { startDetection(); }
- (void)actExtensions:(id)sender { openInChrome(@"chrome://extensions"); }
- (void)actFolder:(id)sender { revealFolder(); }
- (void)actCopy:(id)sender { copyPath(); }
@end

static AppDelegate *delegate = nil;

int main(int argc, char **argv) {
    (void)argc; (void)argv;
    NSApplication *app = [NSApplication sharedApplication];
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    delegate = [AppDelegate new];
    app.delegate = delegate;
    [app activateIgnoringOtherApps:YES];

    NSString *manifestPath = [NSString stringWithFormat:
        @"%s/ChromeExtension/manifest.json", SOURCE_ROOT];
    BOOL installed = [[NSFileManager defaultManager] fileExistsAtPath:manifestPath];
    BOOL chromeMissing = !chromeInstalled();

    if (chromeInstalled() && installed) {
        // 默认模式：Host 就绪判定通过则已直接打开产品页并返回 0。
        int code = run_host_sync(@[@"--launcher-default"]);
        if (code == 0) return 0;
        startDetection();
    }

    // 状态窗：非模态、可关闭、10 分钟截止（D04/D05）。
    NSWindow *win = [[NSWindow alloc]
        initWithContentRect:NSMakeRect(0, 0, 460, 170)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
        backing:NSBackingStoreBuffered defer:NO];
    win.title = @"Natives";
    [win center];
    NSTextField *status = [[NSTextField alloc] initWithFrame:NSMakeRect(16, 96, 428, 56)];
    status.editable = NO; status.bezeled = NO; status.drawsBackground = NO;
    status.stringValue = chromeMissing
        ? @"未找到 Google Chrome。点击\"获取 Chrome\"用默认浏览器打开官方页；安装后点\"重新检测\"。"
        : (installed
            ? @"正在连接扩展（总等待最长 10 分钟）。请在 Chrome 完成开发者模式与\"加载已解压的扩展程序\"，选择\"下载\"文件夹里的 Natives-Extension。"
            : @"安装不完整：缺少随包扩展目录，请重新安装 Natives。");
    statusField = status;
    NSArray *buttons = @[
        @[@"重新检测", @"detect"], @[@"打开扩展管理页", @"extensions"],
        @[@"显示扩展文件夹", @"folder"], @[@"复制目录路径", @"copy"],
    ];
    CGFloat y = 56;
    for (NSArray *button in buttons) {
        NSButton *b = [[NSButton alloc] initWithFrame:NSMakeRect(16, y, 200, 30)];
        b.title = button[0];
        b.bezelStyle = NSBezelStyleRounded;
        b.target = delegate;
        b.action = NSSelectorFromString([NSString stringWithFormat:@"act%@:", button[1]]);
        [win.contentView addSubview:b];
        y -= 36;
    }
    delegate.window = win;
    [win makeKeyAndOrderFront:nil];

    // 首次引导的自动打开动作（§1.4）：指南 + 扩展管理页 + 定位目录。
    if (installed && !chromeMissing) {
        openInChrome(ONBOARDING_PATH);
        openInChrome(@"chrome://extensions");
        revealFolder();
        startDetection();
    } else if (chromeMissing) {
        statusField.stringValue = @"未找到 Google Chrome：点击\"获取 Chrome\"用默认浏览器打开官方页。";
    }

    // 定时器：检测完成/截止判定（真实 10 分钟截止，D04）。
    [NSTimer scheduledTimerWithTimeInterval:0.5 repeats:YES block:^(NSTimer *timer) {
        if (!statusWindow || !statusWindow.isVisible) { [timer invalidate]; return; }
        if (activeTask && !activeTask.isRunning && activeTask.terminationStatus == 0) {
            openProductIfReady();
            [timer invalidate];
            return;
        }
        totalWaited += 0.5;
        if (totalWaited >= TOTAL_DEADLINE_SECONDS && activeTask.isRunning) {
            [activeTask terminate];
            updateStatus(@"已等待 10 分钟仍未检测到连接。完成加载后点击\"重新检测\"。");
        }
    }];

    // 判定已就绪时：默认模式已打开产品；否则保持状态窗等待按钮/URL 动作。
    if (!chromeMissing && installed) {
        // run_launcher_default 返回 2 表示需要引导；状态窗已就位。
    } else if (!installed) {
        updateStatus(@"安装不完整：缺少随包扩展目录，请重新安装 Natives。");
    }
    [app run];
    return 0;
}
