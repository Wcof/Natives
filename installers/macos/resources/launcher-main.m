/* Natives 主入口包装（实施方案 §1.1/§1.3/§1.4）：
 * - 启动 1 秒内出现原生状态窗；等待期间所有按钮可用（D04）
 * - Host 检测以 NSTask 异步运行，主线程不阻塞 waitpid
 * - 真实 10 分钟总截止：到时停止检测，仅"重新检测"开启新一轮
 * - 取消立即终止检测进程并退出；不关闭 Chrome
 * - Chrome 发现覆盖系统与用户应用目录；缺失时用默认浏览器打开官方页
 * - 不自动改剪贴板（仅在用户点击"复制目录路径"时）；无随机 session 展示
 * - 扩展目录别名逻辑单一来源在 Host（--reveal-extension-dir），此处不建链接 */
#import <AppKit/AppKit.h>
#include <spawn.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

#ifndef SOURCE_ROOT
#define SOURCE_ROOT "/Library/Application Support/Natives-Local"
#endif
#ifndef EXTENSION_ID
#define EXTENSION_ID "gehmgcnlpdepnpmcbbdaijabcjdnbfmh"
#endif
#define ONBOARDING_PATH @"/Applications/Natives.app/Contents/Resources/onboarding/index.html"
#define TOTAL_DEADLINE_SECONDS 600

static NSTask *activeTask = nil;

static int run_host_sync(const char **argv) {
    pid_t pid = 0;
    posix_spawnattr_t attr;
    posix_spawnattr_init(&attr);
    int status = -1;
    if (posix_spawn(&pid, argv[0], NULL, &attr, (char *const *)argv, environ) != 0) {
        return 127;
    }
    posix_spawnattr_destroy(&attr);
    while (waitpid(pid, &status, 0) < 0) {
    }
    return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}

static BOOL chromeInstalled(void) {
    return access("/Applications/Google Chrome.app", F_OK) == 0
        || access([NSHomeDirectory() stringByAppendingPathComponent:@"Applications/Google Chrome.app"].fileSystemRepresentation, F_OK) == 0;
}

// Chrome 目标必须显式指定 Chrome（D05 修复：chrome:// scheme 未注册到
// LaunchServices，NSWorkspace openURL 会报"未设定应用程序"；file: 也不能
// 依赖默认浏览器）。/usr/bin/open -a 按名称定位，系统/用户级安装均适用。
static void openInChrome(NSString *target) {
    NSTask *task = [[NSTask alloc] init];
    task.launchPath = @"/usr/bin/open";
    task.arguments = @[@"-a", @"Google Chrome", target];
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) {
        NSLog(@"open in Chrome failed: %@", error);
    }
}

// 异步运行 Host 检测（D04）：NSTask，主线程只轮询状态，保持响应。
static void startHostDetection(NSString *hostPath, NSTimeInterval timeoutSeconds) {
    if (activeTask && activeTask.isRunning) return;
    NSTask *task = [[NSTask alloc] init];
    task.launchPath = hostPath;
    task.arguments = @[@"--launcher-setup", @"--setup-timeout-secs",
                       [NSString stringWithFormat:@"%.0f", timeoutSeconds], @"--no-open"];
    task.standardOutput = [NSPipe pipe];
    task.standardError = [NSPipe pipe];
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) {
        NSLog(@"host detection launch failed: %@", error);
        return;
    }
    activeTask = task;
}

int main(int argc, char **argv) {
    (void)argc;
    (void)argv;
    NSApplication *app = [NSApplication sharedApplication];
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    [app activateIgnoringOtherApps:YES];

    NSString *hostPath = [NSString stringWithFormat:@"%s/native-file-host", SOURCE_ROOT];
    NSString *extensionDir = [NSString stringWithFormat:@"%s/ChromeExtension", SOURCE_ROOT];
    NSString *manifestPath = [extensionDir stringByAppendingString:@"/manifest.json"];

    // 默认入口：Host 就绪判定（近期握手 + 版本匹配 + 系统目录完整）通过时
    // 已直接打开产品页；返回 2 = 需要首次引导（§1.1）。
    const char *default_argv[3] = {[hostPath fileSystemRepresentation], "--launcher-default", NULL};
    int code = run_host_sync(default_argv);
    if (code == 0) return 0;

    if (![[NSFileManager defaultManager] fileExistsAtPath:manifestPath]) {
        NSAlert *alert = [[NSAlert alloc] init];
        alert.alertStyle = NSAlertStyleCritical;
        alert.messageText = @"Natives";
        alert.informativeText = @"安装不完整：缺少随包扩展目录，请重新安装 Natives。\n\nInstaller incomplete: bundled extension folder missing. Reinstall Natives.";
        [alert addButtonWithTitle:@"好"];
        [alert runModal];
        return 1;
    }

    // §1.3 状态职责：打开动作归原生 Launcher；Host 只做有界检测（--no-open）。
    openInChrome(ONBOARDING_PATH);  // file: 离线指南（§1.3）
    openInChrome(@"chrome://extensions");

    NSTimeInterval totalWaited = 0;
    while (YES) {
        // 状态窗立即出现（§1.4：1 秒内有可见反馈），按钮全程可用。
        NSAlert *alert = [[NSAlert alloc] init];
        alert.alertStyle = NSAlertStyleInformational;
        alert.messageText = @"Natives 正在连接扩展";
        BOOL chromeMissing = !chromeInstalled();
        alert.informativeText = chromeMissing
            ? @"未找到 Google Chrome。点击\"获取 Chrome\"用默认浏览器打开官方下载页；安装后点击\"重新检测\"。\n\nGoogle Chrome not found. Get Chrome opens the official page in your default browser."
            : @"请在 Chrome 中开启开发者模式并\"加载已解压的扩展程序\"（目录已在访达中定位）。加载后本窗口会自动确认；总等待最长 10 分钟，之后可重新检测。\n\nFinish Developer Mode and Load unpacked in Chrome. This window confirms automatically; detection stops after 10 minutes and can be re-run.";
        if (chromeMissing) {
            [alert addButtonWithTitle:@"获取 Chrome"];
        } else {
            [alert addButtonWithTitle:@"重新检测"];
        }
        [alert addButtonWithTitle:@"打开扩展管理页"];
        [alert addButtonWithTitle:@"显示扩展文件夹"];
        [alert addButtonWithTitle:@"复制目录路径"];
        [alert addButtonWithTitle:@"取消"];

        if (!chromeMissing) {
            startHostDetection(hostPath, MIN(TOTAL_DEADLINE_SECONDS - totalWaited, 60));
        }

        // 模态运行但每 0.2s 回到主循环检查：截止一到终止检测进程（D04）。
        NSDate *attemptStart = [NSDate date];
        while (YES) {
            if (activeTask && !activeTask.isRunning) break;
            if (chromeMissing) break;
            if (-[attemptStart timeIntervalSinceNow] >= 60 && activeTask.isRunning) {
                [activeTask terminate];
            }
            [NSThread sleepForTimeInterval:0.05];
            [[NSRunLoop mainRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.15]];
            totalWaited += 0.2;
            if (totalWaited >= TOTAL_DEADLINE_SECONDS) {
                if (activeTask.isRunning) [activeTask terminate];
                break;
            }
        }
        BOOL verified = activeTask && activeTask.terminationStatus == 0 && !chromeMissing;
        if (verified) {
            openInChrome([NSString stringWithFormat:
                @"chrome-extension://%s/space.html", EXTENSION_ID]);
            return 0; // 成功交接：Launcher 退出，应用图标保留。
        }
        // 结果窗：超时或未连接，用户选择下一步；取消结束本次引导。
        NSAlert *resultAlert = [[NSAlert alloc] init];
        resultAlert.alertStyle = NSAlertStyleInformational;
        resultAlert.messageText = @"Natives";
        resultAlert.informativeText = totalWaited >= TOTAL_DEADLINE_SECONDS
            ? @"已等待 10 分钟仍未检测到扩展连接。请确认扩展已加载后点击\"重新检测\"。\n\nNot connected after 10 minutes. Press Re-detect once the extension is loaded."
            : @"尚未检测到扩展连接。请完成加载后点击\"重新检测\"，或取消稍后再试。\n\nNot connected yet. Press Re-detect after loading, or cancel and retry later.";
        [resultAlert addButtonWithTitle:@"重新检测"];
        [resultAlert addButtonWithTitle:@"打开扩展管理页"];
        [resultAlert addButtonWithTitle:@"显示扩展文件夹"];
        [resultAlert addButtonWithTitle:@"复制目录路径"];
        [resultAlert addButtonWithTitle:@"取消"];
        NSModalResponse response = [resultAlert runModal];
        NSModalResponse first = NSAlertFirstButtonReturn;
        if (response == first) {
            continue; // 重新检测：开启新一轮有界等待。
        }
        if (response == first + 1) {
            openInChrome(@"chrome://extensions");
        } else if (response == first + 2) {
            [[NSWorkspace sharedWorkspace] openURL:[NSURL fileURLWithPath:extensionDir]
                                           options:NSWorkspaceLaunchWithoutActivation
                                     configuration:@{} error:NULL];
        } else if (response == first + 3) {
            [[NSPasteboard generalPasteboard] clearContents];
            [[NSPasteboard generalPasteboard] setString:extensionDir forType:NSPasteboardTypeString];
        } else {
            return 0; // 取消：结束本次引导；应用图标保留，可再次双击。
        }
    }
}
