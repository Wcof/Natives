/* Natives 主入口包装（用户流程优化版）：
 * 1. 启动时先判断 Chrome 中是否已安装该扩展；
 * 2. 若已装载（正常启动）：直接在 Chrome 打开产品页进入系统；
 * 3. 若未装载：显示置顶步骤型弹窗：
 *    - 步骤 1：提示在 Chrome 开启开发者模式；提供【退出】与【打开 Chrome 扩展页面】按钮；
 *      点击后在 Chrome 打开扩展页，并切到步骤 2；
 *    - 步骤 2：自动在访达中高亮选中扩展目录，提示直接拖拽装载；提供【重新打开扩展文件夹】与【完成】按钮；
 *      点击【完成】直接关闭应用退出；
 * 4. 彻底不碰 ~/Downloads 目录，绝不触发下载权限弹窗。 */
#import <AppKit/AppKit.h>
#include <spawn.h>
#include <stdlib.h>
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
    [task launchAndReturnError:nil];
}

// 直接在访达中高亮选中真实扩展目录，绝不碰 ~/Downloads，零权限索取
static void revealRealExtensionFolder(void) {
    NSString *extDir = [NSString stringWithFormat:@"%s/ChromeExtension", SOURCE_ROOT];
    NSURL *targetURL = [NSURL fileURLWithPath:extDir];
    [[NSWorkspace sharedWorkspace] activateFileViewerSelectingURLs:@[targetURL]];

    NSTask *task = [[NSTask alloc] init];
    task.launchPath = @"/usr/bin/osascript";
    task.arguments = @[@"-e", @"tell application \"Finder\" to activate"];
    [task launchAndReturnError:nil];
}

// 步骤 1：判断 Chrome 中是否已经安装了该扩展
static BOOL isExtensionInstalledInChrome(void) {
    // 1. 检查 Natives 握手标记记录
    NSString *handshakePath = [NSHomeDirectory() stringByAppendingPathComponent:@".natives/extensions/chrome/handshake.json"];
    if ([[NSFileManager defaultManager] fileExistsAtPath:handshakePath]) {
        return YES;
    }

    // 2. 检查 Chrome 各 Profile 下的 Preferences 是否登记了该扩展 ID
    NSString *chromeBase = [NSHomeDirectory() stringByAppendingPathComponent:@"Library/Application Support/Google/Chrome"];
    NSArray *profiles = @[@"Default", @"Profile 1", @"Profile 2", @"Profile 3", @"Profile 4", @"Profile 5"];
    NSString *targetId = [NSString stringWithUTF8String:EXTENSION_ID];
    for (NSString *profile in profiles) {
        NSString *prefPath = [chromeBase stringByAppendingPathComponent:[NSString stringWithFormat:@"%@/Preferences", profile]];
        NSData *data = [NSData dataWithContentsOfFile:prefPath];
        if (data) {
            NSDictionary *json = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
            if ([json isKindOfClass:[NSDictionary class]]) {
                NSDictionary *settings = json[@"extensions"][@"settings"];
                if (settings[targetId] != nil) {
                    return YES;
                }
            }
        }
    }

    // 3. 检查 Chromium 各 Profile
    NSString *chromiumBase = [NSHomeDirectory() stringByAppendingPathComponent:@"Library/Application Support/Chromium"];
    for (NSString *profile in profiles) {
        NSString *prefPath = [chromiumBase stringByAppendingPathComponent:[NSString stringWithFormat:@"%@/Preferences", profile]];
        NSData *data = [NSData dataWithContentsOfFile:prefPath];
        if (data) {
            NSDictionary *json = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
            if ([json isKindOfClass:[NSDictionary class]]) {
                NSDictionary *settings = json[@"extensions"][@"settings"];
                if (settings[targetId] != nil) {
                    return YES;
                }
            }
        }
    }

    // 4. 检查 Host 判定
    int code = run_host_sync(@[@"--launcher-default", @"--no-open"]);
    if (code == 0) {
        return YES;
    }

    return NO;
}

@interface WizardController : NSObject <NSWindowDelegate>
@property(strong) NSWindow *window;
@property(strong) NSView *step1View;
@property(strong) NSView *step2View;
@end

@implementation WizardController

- (void)setupUI {
    NSRect frame = NSMakeRect(0, 0, 500, 240);
    self.window = [[NSWindow alloc] initWithContentRect:frame
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
        backing:NSBackingStoreBuffered defer:NO];
    self.window.title = @"Natives";
    [self.window center];
    self.window.delegate = self;

    // 始终置顶（最上层图层，不被任何应用遮挡）
    self.window.level = NSFloatingWindowLevel;
    self.window.hidesOnDeactivate = NO;

    [self createStep1View:frame];
    [self createStep2View:frame];

    [self.window.contentView addSubview:self.step1View];
    [self.window makeKeyAndOrderFront:nil];
    [self.window orderFrontRegardless];
}

// ── 步骤 1 View ───────────────────────────────────────────
- (void)createStep1View:(NSRect)frame {
    self.step1View = [[NSView alloc] initWithFrame:frame];

    // 图标
    NSImageView *iconView = [[NSImageView alloc] initWithFrame:NSMakeRect(24, 154, 56, 56)];
    NSString *iconPath = [[NSBundle mainBundle] pathForResource:@"natives" ofType:@"icns"];
    NSImage *appIcon = iconPath ? [[NSImage alloc] initWithContentsOfFile:iconPath] : nil;
    if (!appIcon) appIcon = [NSImage imageNamed:NSImageNameApplicationIcon];
    iconView.image = appIcon;
    [self.step1View addSubview:iconView];

    // 标题与步骤指示
    NSTextField *title = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 178, 380, 26)];
    title.editable = NO; title.bezeled = NO; title.drawsBackground = NO;
    title.font = [NSFont boldSystemFontOfSize:17];
    title.stringValue = @"Natives 扩展安装引导 (步骤 1/2)";
    [self.step1View addSubview:title];

    NSTextField *desc = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 106, 380, 64)];
    desc.editable = NO; desc.bezeled = NO; desc.drawsBackground = NO;
    desc.font = [NSFont systemFontOfSize:13.5];
    desc.textColor = [NSColor secondaryLabelColor];
    desc.stringValue = @"未在 Chrome 中检测到已装载的 Natives 扩展。\n\n请点击下方按钮打开 Chrome 扩展管理页，并在页面右上角开启「开发者模式」开关。";
    [self.step1View addSubview:desc];

    // 分割线
    NSBox *sep = [[NSBox alloc] initWithFrame:NSMakeRect(24, 75, 452, 1)];
    sep.boxType = NSBoxSeparator;
    [self.step1View addSubview:sep];

    // 按钮 1：【退出】
    NSButton *btnExit = [[NSButton alloc] initWithFrame:NSMakeRect(24, 22, 120, 32)];
    btnExit.title = @"退出";
    btnExit.bezelStyle = NSBezelStyleRounded;
    btnExit.target = self;
    btnExit.action = @selector(actExit);
    [self.step1View addSubview:btnExit];

    // 按钮 2：【打开 Chrome 扩展页面】(Primary)
    NSButton *btnOpen = [[NSButton alloc] initWithFrame:NSMakeRect(246, 22, 230, 32)];
    btnOpen.title = @"打开 Chrome 扩展页面";
    btnOpen.bezelStyle = NSBezelStyleRounded;
    btnOpen.keyEquivalent = @"\r"; // 回车
    btnOpen.target = self;
    btnOpen.action = @selector(actStep1Next);
    [self.step1View addSubview:btnOpen];
}

// ── 步骤 2 View ───────────────────────────────────────────
- (void)createStep2View:(NSRect)frame {
    self.step2View = [[NSView alloc] initWithFrame:frame];

    NSImageView *iconView = [[NSImageView alloc] initWithFrame:NSMakeRect(24, 154, 56, 56)];
    NSString *iconPath = [[NSBundle mainBundle] pathForResource:@"natives" ofType:@"icns"];
    NSImage *appIcon = iconPath ? [[NSImage alloc] initWithContentsOfFile:iconPath] : nil;
    if (!appIcon) appIcon = [NSImage imageNamed:NSImageNameApplicationIcon];
    iconView.image = appIcon;
    [self.step2View addSubview:iconView];

    NSTextField *title = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 178, 380, 26)];
    title.editable = NO; title.bezeled = NO; title.drawsBackground = NO;
    title.font = [NSFont boldSystemFontOfSize:17];
    title.stringValue = @"装载扩展文件夹 (步骤 2/2)";
    [self.step2View addSubview:title];

    NSTextField *desc = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 98, 380, 72)];
    desc.editable = NO; desc.bezeled = NO; desc.drawsBackground = NO;
    desc.font = [NSFont systemFontOfSize:13.5];
    desc.textColor = [NSColor labelColor];
    desc.stringValue = @"已在访达中为您高亮选中 ChromeExtension 文件夹。\n\n👉 请直接将该文件夹拖入 Chrome 扩展页面中即可秒速完成装载！";
    [self.step2View addSubview:desc];

    NSBox *sep = [[NSBox alloc] initWithFrame:NSMakeRect(24, 75, 452, 1)];
    sep.boxType = NSBoxSeparator;
    [self.step2View addSubview:sep];

    // 重新打开文件夹按钮
    NSButton *btnReopen = [[NSButton alloc] initWithFrame:NSMakeRect(24, 22, 190, 32)];
    btnReopen.title = @"重新打开扩展文件夹";
    btnReopen.bezelStyle = NSBezelStyleRounded;
    btnReopen.target = self;
    btnReopen.action = @selector(actReopenFolder);
    [self.step2View addSubview:btnReopen];

    // 【完成】按钮：关闭 APP
    NSButton *btnFinish = [[NSButton alloc] initWithFrame:NSMakeRect(346, 22, 130, 32)];
    btnFinish.title = @"完成";
    btnFinish.bezelStyle = NSBezelStyleRounded;
    btnFinish.keyEquivalent = @"\r";
    btnFinish.target = self;
    btnFinish.action = @selector(actFinish);
    [self.step2View addSubview:btnFinish];
}

- (void)actExit {
    [NSApp terminate:nil];
}

// 步骤 1 完成：打开 Chrome 扩展页面并切到步骤 2
- (void)actStep1Next {
    openInChrome(@"chrome://extensions");
    [self.step1View removeFromSuperview];
    [self.window.contentView addSubview:self.step2View];

    // 进入步骤 2 自动高亮扩展文件夹
    revealRealExtensionFolder();
}

- (void)actReopenFolder {
    revealRealExtensionFolder();
}

- (void)actFinish {
    // 点击完成，自动退出 APP
    [NSApp terminate:nil];
}

- (void)windowWillClose:(NSNotification *)note {
    [NSApp terminate:nil];
}

@end

int main(int argc, char **argv) {
    (void)argc; (void)argv;
    NSApplication *app = [NSApplication sharedApplication];
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    [app activateIgnoringOtherApps:YES];

    NSString *manifestPath = [NSString stringWithFormat:@"%s/ChromeExtension/manifest.json", SOURCE_ROOT];
    BOOL manifestInstalled = [[NSFileManager defaultManager] fileExistsAtPath:manifestPath];

    if (!manifestInstalled) {
        NSAlert *alert = [[NSAlert alloc] init];
        alert.alertStyle = NSAlertStyleCritical;
        alert.messageText = @"Natives";
        alert.informativeText = @"安装不完整：缺少系统扩展目录，请重新安装 Natives。";
        [alert addButtonWithTitle:@"好"];
        [alert runModal];
        return 1;
    }

    // 1. 判断 Chrome 中是否已经安装了该扩展
    if (chromeInstalled() && isExtensionInstalledInChrome()) {
        // 8. 已经安装：正常启动，直接在 Chrome 中打开空间产品页！
        openInChrome([NSString stringWithFormat:@"chrome-extension://%s/space.html", EXTENSION_ID]);
        return 0;
    }

    // 3. 未安装：打开置顶步骤型弹窗（步骤 1/2）
    WizardController *wizard = [WizardController new];
    [wizard setupUI];
    [app run];
    return 0;
}
