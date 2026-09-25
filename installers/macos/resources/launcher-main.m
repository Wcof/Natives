/* Natives 主入口包装（§1.1/§1.4，D03/D04 整改版）：
 * - 双击后立即显示原生状态窗，主线程不执行同步 waitpid（D04）；
 * - 入口三态判定交给 native-file-host（--launcher-default）：版本匹配的
 *   握手 + 产品配置状态，包装进程不信任旧握手文件、不读取 Chrome 私有
 *   profile Preferences（D03，§1.4 隐私边界）；
 * - Host 调用全程异步（NSTask terminationHandler），窗口始终可取消；
 *   单次检测总截止 600 秒，超时终止等待并提供"重新检测/退出"；
 * - 未就绪时进入两步引导（打开扩展管理页 + 访达定位固定目录），
 *   步骤 2 提供"重新检测"接续；成功后由 Host 打开产品页并退出。
 * 不碰 ~/Downloads，不弹 Terminal，不创建常驻检测服务。 */
#import <AppKit/AppKit.h>
#import "NativesStatusBar.h"
#include <stdlib.h>
#include <unistd.h>

#ifndef SOURCE_ROOT
#define SOURCE_ROOT "/Library/Application Support/Natives-Local"
#endif
#ifndef EXTENSION_ID
#define EXTENSION_ID "gehmgcnlpdepnpmcbbdaijabcjdnbfmh"
#endif
#ifndef SETUP_SCHEME
#define SETUP_SCHEME "natives-setup-local"
#endif

static NSTask *volatile hostTask = nil;

/// 异步运行 Host；完成回调在主线程执行。取消 = terminate 当前任务。
static void runHostAsync(NSArray<NSString *> *args,
                         void (^onExit)(int code, NSString *stdoutText)) {
    NSTask *task = [[NSTask alloc] init];
    task.launchPath = [NSString stringWithFormat:@"%s/native-file-host", SOURCE_ROOT];
    task.arguments = args;
    NSPipe *outPipe = [NSPipe pipe];
    task.standardOutput = outPipe;
    task.standardError = [NSPipe pipe];
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) {
        dispatch_async(dispatch_get_main_queue(), ^{ onExit(127, @""); });
        return;
    }
    hostTask = task;
    task.terminationHandler = ^(NSTask *finished) {
        // 进程已退出，管道 EOF 已就绪；同步读取不会阻塞界面。
        NSData *data = [[outPipe fileHandleForReading] readDataToEndOfFile];
        NSString *text = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] ?: @"";
        dispatch_async(dispatch_get_main_queue(), ^{
            if (hostTask == finished) hostTask = nil;
            onExit(finished.terminationStatus, text);
        });
    };
}

static void cancelHostWait(void) {
    NSTask *task = hostTask;
    hostTask = nil;
    [task terminate];
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

// 在访达中高亮选中真实扩展目录（固定系统源，绝不碰 ~/Downloads）
static void revealRealExtensionFolder(void) {
    NSString *extDir = [NSString stringWithFormat:@"%s/ChromeExtension", SOURCE_ROOT];
    NSURL *targetURL = [NSURL fileURLWithPath:extDir];
    [[NSWorkspace sharedWorkspace] activateFileViewerSelectingURLs:@[targetURL]];
    NSTask *task = [[NSTask alloc] init];
    task.launchPath = @"/usr/bin/osascript";
    task.arguments = @[@"-e", @"tell application \"Finder\" to activate"];
    [task launchAndReturnError:nil];
}

/// 从 Host stdout JSON 提取 step 字段。
static NSString *stepFromOutput(NSString *output) {
    NSData *data = [output dataUsingEncoding:NSUTF8StringEncoding];
    if (!data) return nil;
    id json = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if ([json isKindOfClass:[NSDictionary class]]) {
        id step = json[@"step"];
        if ([step isKindOfClass:[NSString class]]) return step;
    }
    return nil;
}

@class StatusController;
@class WizardController;

static StatusController *sharedStatus = nil;

/// 检测状态窗：spinner 文案 + 取消按钮；600 秒总截止（D04：真正停止）。
@interface StatusController : NSObject <NSWindowDelegate>
@property(strong) NSWindow *window;
@property(strong) NSTextField *statusText;
@property(strong) NSButton *btnCancel;
@property(strong) NSButton *btnRetry;
@property() dispatch_block_t deadline;
@end

/// 两步引导：步骤 1 打开扩展管理页；步骤 2 访达定位 + 重新检测接续。
@interface WizardController : NSObject <NSWindowDelegate>
@property(strong) NSWindow *window;
@property(strong) NSView *step1View;
@property(strong) NSView *step2View;
- (void)setupUI;
@end

@implementation StatusController

- (void)setupUI {
    NSRect frame = NSMakeRect(0, 0, 440, 150);
    self.window = [[NSWindow alloc] initWithContentRect:frame
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
        backing:NSBackingStoreBuffered defer:NO];
    self.window.title = @"Natives";
    [self.window center];
    self.window.delegate = self;
    self.window.level = NSFloatingWindowLevel;
    self.window.hidesOnDeactivate = NO;

    NSImageView *iconView = [[NSImageView alloc] initWithFrame:NSMakeRect(24, 62, 56, 56)];
    NSString *iconPath = [[NSBundle mainBundle] pathForResource:@"natives" ofType:@"icns"];
    NSImage *appIcon = iconPath ? [[NSImage alloc] initWithContentsOfFile:iconPath] : nil;
    if (!appIcon) appIcon = [NSImage imageNamed:NSImageNameApplicationIcon];
    iconView.image = appIcon;
    [self.window.contentView addSubview:iconView];

    self.statusText = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 84, 320, 40)];
    self.statusText.editable = NO; self.statusText.bezeled = NO; self.statusText.drawsBackground = NO;
    self.statusText.font = [NSFont systemFontOfSize:13.5];
    self.statusText.stringValue = @"正在检测 Natives 扩展状态…";
    [self.window.contentView addSubview:self.statusText];

    self.btnCancel = [[NSButton alloc] initWithFrame:NSMakeRect(24, 18, 110, 30)];
    self.btnCancel.title = @"退出";
    self.btnCancel.bezelStyle = NSBezelStyleRounded;
    self.btnCancel.target = self;
    self.btnCancel.action = @selector(actExit);
    [self.window.contentView addSubview:self.btnCancel];

    self.btnRetry = [[NSButton alloc] initWithFrame:NSMakeRect(306, 18, 110, 30)];
    self.btnRetry.title = @"重新检测";
    self.btnRetry.bezelStyle = NSBezelStyleRounded;
    self.btnRetry.keyEquivalent = @"\r";
    self.btnRetry.target = self;
    self.btnRetry.action = @selector(actRetry);
    self.btnRetry.hidden = YES;
    [self.window.contentView addSubview:self.btnRetry];

    [self.window makeKeyAndOrderFront:nil];
    [self.window orderFrontRegardless];
}

- (void)startDetection {
    self.btnCancel.hidden = NO;
    self.btnRetry.hidden = YES;
    self.statusText.stringValue = @"正在检测 Natives 扩展状态…";
    // 非 MRC 环境不用 __weak；block 持有 self 直到检测结束/取消，
    // 进程生命周期短暂，无泄漏风险。
    __block StatusController *blockSelf = self;
    // 单次检测总截止：600 秒后终止 Host 等待（D04：真正停止，不循环续期）。
    self.deadline = dispatch_block_create(DISPATCH_BLOCK_INHERIT_QOS_CLASS, ^{
        [blockSelf hostTimedOut];
    });
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)600 * NSEC_PER_SEC),
                   dispatch_get_main_queue(), self.deadline);
    runHostAsync(@[@"--launcher-default"], ^(int code, NSString *output) {
        [blockSelf hostFinished:code output:output];
    });
}

- (void)hostFinished:(int)code output:(NSString *)output {
    if (self.deadline) { dispatch_block_cancel(self.deadline); self.deadline = nil; }
    NSString *step = stepFromOutput(output);
    if (code == 0 && ([step isEqualToString:@"opened_natives"] || [step isEqualToString:@"opened_config"])) {
        // Host 已打开产品页/配置页；成功交接后 Launcher 退出。
        [NSApp terminate:nil];
        return;
    }
    if (code == 2 && [step isEqualToString:@"setup_required"]) {
        [self showWizard];
        return;
    }
    // 错误/超时：如实显示，可重试（§1.4 错误可见）。
    self.btnCancel.hidden = YES;
    self.btnRetry.hidden = NO;
    if ([step isEqualToString:@"setup_required"] || code == 2) {
        self.statusText.stringValue = @"尚未检测到已加载的 Natives 扩展。请完成扩展加载后重新检测。";
    } else {
        self.statusText.stringValue = @"检测未完成。请确认已安装 Google Chrome，然后重新检测。";
    }
}

- (void)hostTimedOut {
    cancelHostWait();
    self.btnCancel.hidden = YES;
    self.btnRetry.hidden = NO;
    self.statusText.stringValue = @"检测超时。请确认 Chrome 已打开且扩展已加载，然后重新检测。";
}

- (void)actRetry {
    if (!chromeInstalled()) {
        self.statusText.stringValue = @"未找到 Google Chrome。请安装后重新检测。";
        openInChrome(@"https://www.google.com/chrome/");
        return;
    }
    [self startDetection];
}

- (void)actExit {
    [NSApp terminate:nil];
}

- (void)showWizard {
    WizardController *wizard = [[WizardController alloc] init];
    [wizard setupUI];
    [self.window close];
}

- (void)windowWillClose:(NSNotification *)note {
    [NSApp terminate:nil];
}

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
    self.window.level = NSFloatingWindowLevel;
    self.window.hidesOnDeactivate = NO;

    [self createStep1View:frame];
    [self createStep2View:frame];

    [self.window.contentView addSubview:self.step1View];
    [self.window makeKeyAndOrderFront:nil];
    [self.window orderFrontRegardless];
}

- (NSView *)newStepHeaderView:(NSRect)frame title:(NSString *)titleText desc:(NSString *)descText {
    NSView *view = [[NSView alloc] initWithFrame:frame];

    NSImageView *iconView = [[NSImageView alloc] initWithFrame:NSMakeRect(24, 154, 56, 56)];
    NSString *iconPath = [[NSBundle mainBundle] pathForResource:@"natives" ofType:@"icns"];
    NSImage *appIcon = iconPath ? [[NSImage alloc] initWithContentsOfFile:iconPath] : nil;
    if (!appIcon) appIcon = [NSImage imageNamed:NSImageNameApplicationIcon];
    iconView.image = appIcon;
    [view addSubview:iconView];

    NSTextField *title = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 178, 380, 26)];
    title.editable = NO; title.bezeled = NO; title.drawsBackground = NO;
    title.font = [NSFont boldSystemFontOfSize:17];
    title.stringValue = titleText;
    [view addSubview:title];

    NSTextField *desc = [[NSTextField alloc] initWithFrame:NSMakeRect(92, 98, 380, 72)];
    desc.editable = NO; desc.bezeled = NO; desc.drawsBackground = NO;
    desc.font = [NSFont systemFontOfSize:13.5];
    desc.textColor = [NSColor secondaryLabelColor];
    desc.stringValue = descText;
    [view addSubview:desc];

    NSBox *sep = [[NSBox alloc] initWithFrame:NSMakeRect(24, 75, 452, 1)];
    sep.boxType = NSBoxSeparator;
    [view addSubview:sep];
    return view;
}

- (void)createStep1View:(NSRect)frame {
    self.step1View = [self newStepHeaderView:frame
        title:@"Natives 扩展安装引导 (步骤 1/2)"
        desc:@"未在 Chrome 中检测到已装载的 Natives 扩展。\n\n请点击下方按钮打开 Chrome 扩展管理页，并在页面右上角开启「开发者模式」开关。"];

    NSButton *btnExit = [[NSButton alloc] initWithFrame:NSMakeRect(24, 22, 120, 32)];
    btnExit.title = @"退出";
    btnExit.bezelStyle = NSBezelStyleRounded;
    btnExit.target = self;
    btnExit.action = @selector(actExit);
    [self.step1View addSubview:btnExit];

    NSButton *btnOpen = [[NSButton alloc] initWithFrame:NSMakeRect(246, 22, 230, 32)];
    btnOpen.title = @"打开 Chrome 扩展页面";
    btnOpen.bezelStyle = NSBezelStyleRounded;
    btnOpen.keyEquivalent = @"\r";
    btnOpen.target = self;
    btnOpen.action = @selector(actStep1Next);
    [self.step1View addSubview:btnOpen];
}

- (void)createStep2View:(NSRect)frame {
    self.step2View = [self newStepHeaderView:frame
        title:@"装载扩展文件夹 (步骤 2/2)"
        desc:@"已在访达中为您高亮选中 ChromeExtension 文件夹。\n\n👉 请直接将该文件夹拖入 Chrome 扩展页面中即可秒速完成装载！"];

    NSButton *btnReopen = [[NSButton alloc] initWithFrame:NSMakeRect(24, 22, 190, 32)];
    btnReopen.title = @"重新打开扩展文件夹";
    btnReopen.bezelStyle = NSBezelStyleRounded;
    btnReopen.target = self;
    btnReopen.action = @selector(actReopenFolder);
    [self.step2View addSubview:btnReopen];

    // 重新检测：加载完成后接续入口三态判定（§1.4）。
    NSButton *btnRedetect = [[NSButton alloc] initWithFrame:NSMakeRect(226, 22, 110, 32)];
    btnRedetect.title = @"重新检测";
    btnRedetect.bezelStyle = NSBezelStyleRounded;
    btnRedetect.target = self;
    btnRedetect.action = @selector(actRedetect);
    [self.step2View addSubview:btnRedetect];

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

- (void)actStep1Next {
    openInChrome(@"chrome://extensions");
    [self.step1View removeFromSuperview];
    [self.window.contentView addSubview:self.step2View];
    revealRealExtensionFolder();
}

- (void)actReopenFolder {
    revealRealExtensionFolder();
}

- (void)actRedetect {
    // 接续检测：成功（Host 打开产品页）后退出；仍未就绪回到状态窗重试。
    sharedStatus = [StatusController new];
    [sharedStatus setupUI];
    [sharedStatus startDetection];
    [self.window close];
}

- (void)actFinish {
    [NSApp terminate:nil];
}

- (void)windowWillClose:(NSNotification *)note {
    [NSApp terminate:nil];
}

@end

int main(int argc, char **argv) {
    NSApplication *app = [NSApplication sharedApplication];

    // 检查是否作为常驻顶栏/菜单栏启动
    BOOL statusBarOnly = NO;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--status-bar") == 0 || strcmp(argv[i], "--menu-bar") == 0) {
            statusBarOnly = YES;
            break;
        }
    }

    if (statusBarOnly) {
        // 启动清场：干掉同二进制的旧实例再拉起，避免多托盘图标与僵尸面板；
        // 注册退出清场：菜单退出/关窗/交接完成等正常退出路径兜底清扫
        NativesStatusBarSweepProjectProcesses();
        atexit(NativesStatusBarSweepProjectProcesses);
        // 作为无 Dock 图标的系统菜单栏常驻应用运行
        [app setActivationPolicy:NSApplicationActivationPolicyAccessory];
        [[NativesStatusBar sharedBar] setupStatusItem];
        [app run];
        return 0;
    }

    [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    [app activateIgnoringOtherApps:YES];

    // 启动同时挂载菜单栏组件，供日常状态排版预览
    [[NativesStatusBar sharedBar] setupStatusItem];

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

    // 入口判定交给 Host（异步）：版本匹配握手 + 产品配置三态；
    // 包装进程不读旧握手文件、不读 Chrome 私有 profile（D03）。
    sharedStatus = [StatusController new];
    [sharedStatus setupUI];
    [sharedStatus startDetection];
    [app run];
    return 0;
}
