/* Natives 主入口包装（实施方案 §1.1/§1.3）：Finder 双击 Natives.app 时
 * 由 LaunchServices 启动。职责边界（§1.3 状态职责表）：打开 Chrome、
 * 打开/定位 chrome://extensions 和固定扩展目录、复制目录路径、展示
 * 实时检测/错误（原生窗口）并聚焦已有窗口；开发者模式与"加载已解压"
 * 由用户按浏览器要求完成。不承载业务 UI、不持有 Secret、无第二套安装
 * 事务、无常驻进程——有界检测后退出。 */
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
#define ONBOARDING_PATH "/Applications/Natives.app/Contents/Resources/onboarding/index.html"

static int run_host(const char **argv) {
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

static void open_with_chrome(const char *target) {
    char cmd[1024];
    snprintf(cmd, sizeof(cmd), "/usr/bin/open -a 'Google Chrome' '%s'", target);
    int _ = system(cmd);
    (void)_;
}

static void reveal_extension_dir(const char *dir) {
    // 用户可见快捷方式：~/Downloads/Natives-Extension → 固定系统源目录。
    // 只导航不改加载位置；同名真实条目存在时跳过（不覆盖用户数据）。
    char cmd[1024];
    snprintf(cmd, sizeof(cmd),
        "ln -sfn '%s' \"$HOME/Downloads/Natives-Extension\" 2>/dev/null || true", dir);
    int rc_alias = system(cmd);
    (void)rc_alias;
    snprintf(cmd, sizeof(cmd), "/usr/bin/open -R \"$HOME/Downloads/Natives-Extension\"");
    int rc_reveal = system(cmd);
    (void)rc_reveal;
    // §1.1 复制目录路径动作：固定安装目录文本。
    snprintf(cmd, sizeof(cmd), "printf '%%s' '%s' | /usr/bin/pbcopy", dir);
    int _2 = system(cmd);
    (void)_2;
}

static void copy_extension_dir(const char *dir) {
    char cmd[1024];
    snprintf(cmd, sizeof(cmd), "printf '%%s' '%s' | /usr/bin/pbcopy", dir);
    int _ = system(cmd);
    (void)_;
}

int main(int argc, char **argv) {
    (void)argc;
    (void)argv;
    NSApplication *app = [NSApplication sharedApplication];
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    [app activateIgnoringOtherApps:YES];

    const char *host = SOURCE_ROOT "/native-file-host";
    const char *extensionDir = SOURCE_ROOT "/ChromeExtension";

    // 默认入口：Host 注册核心 NM（系统注册已存在时跳过）并在就绪时直接
    // 打开产品页；返回 2 = setup_required（首次/未连接）。
    const char *default_argv[3] = {host, "--launcher-default", NULL};
    int code = run_host(default_argv);
    if (code == 0) return 0;
    if (code != 2) {
        NSAlert *alert = [[NSAlert alloc] init];
        alert.alertStyle = NSAlertStyleCritical;
        alert.messageText = @"Natives";
        alert.informativeText = @"启动引导失败，请重新安装或修复 Natives。\n\nLauncher failed to start; reinstall or repair Natives.";
        [alert runModal];
        return code;
    }

    // 首次引导（§1.1）：打开 Chrome + 离线指南 + 扩展管理页 + Finder 定位，
    // 目录路径复制；随后有界等待（§1.1：沿用最长十分钟限制，超时提供重试）。
    if (access(ONBOARDING_PATH, F_OK) != 0) {
        NSAlert *alert = [[NSAlert alloc] init];
        alert.alertStyle = NSAlertStyleCritical;
        alert.messageText = @"Natives";
        alert.informativeText = @"安装不完整：缺少随包引导资源，请重新安装 Natives。\n\nInstaller incomplete: onboarding resources missing. Reinstall Natives.";
        [alert addButtonWithTitle:@"好"];
        [alert runModal];
        return 1;
    }
    BOOL chromeMissing = (access("/Applications/Google Chrome.app", F_OK) != 0);
    unsigned char session[8];
    arc4random_buf(session, sizeof(session));
    char sessionHex[17];
    for (int i = 0; i < 8; i++) snprintf(sessionHex + i * 2, 3, "%02x", session[i]);
    sessionHex[16] = '\0';
    char onboardingUrl[512];
    snprintf(onboardingUrl, sizeof(onboardingUrl), "file://" ONBOARDING_PATH "?session=%s", sessionHex);
    // §1.3 seam：Launcher → local onboarding URL + session id（仅文案关联）。

    time_t started = time(NULL);
    BOOL first = YES;
    for (;;) {
        if (!chromeMissing) {
            if (first) {
                // §1.3：先打开离线指南，再扩展管理页与目录定位。
                open_with_chrome(onboardingUrl);
                open_with_chrome("chrome://extensions");
                reveal_extension_dir(extensionDir);
                first = NO;
            }
            const char *setup_argv[6] = {host, "--launcher-setup", "--setup-timeout-secs", "60", "--no-open", NULL};
            code = run_host(setup_argv);
            if (code == 0) {
                open_with_chrome("chrome-extension://" EXTENSION_ID "/space.html");
                return 0; // 成功交接，Launcher 退出；应用图标保留。
            }
        }
        BOOL timeout = (time(NULL) - started) > 600;
        BOOL chromeNowMissing = (access("/Applications/Google Chrome.app", F_OK) != 0);
        NSAlert *alert = [[NSAlert alloc] init];
        alert.alertStyle = NSAlertStyleInformational;
        alert.messageText = @"Natives";
        if (chromeNowMissing) {
            alert.informativeText = @"需要 Google Chrome。请从 google.com/chrome 获取官方 Chrome。\n\nNatives requires Google Chrome.";
            [alert addButtonWithTitle:@"重新检测"];
            [alert addButtonWithTitle:@"打开获取页面"];
            [alert addButtonWithTitle:@"取消"];
        } else {
            alert.informativeText = timeout
                ? @"尚未检测到扩展连接（已超过 10 分钟）。完成加载后点击\"重新检测\"。\n\nNot connected yet (over 10 minutes). Press Re-detect after loading."
                : @"尚未检测到扩展连接。请在 Chrome 完成开发者模式与\"加载已解压的扩展程序\"，目录路径已复制。\n\nNot connected yet. Finish Developer Mode and Load unpacked; the folder path is copied.";
            // §1.1 引导窗口按钮。
            [alert addButtonWithTitle:@"重新检测"];
            [alert addButtonWithTitle:@"打开扩展管理页"];
            [alert addButtonWithTitle:@"显示扩展文件夹"];
            [alert addButtonWithTitle:@"复制目录路径"];
            [alert addButtonWithTitle:@"取消"];
        }
        // AppKit 按钮返回码 = NSModalResponseFirstButton + 索引；Esc/关闭
        // 不触发任何按钮（返回 -1000），视为取消：结束引导，图标保留。
        NSModalResponse rc = [alert runModal];
        NSModalResponse firstResponse = NSAlertFirstButtonReturn;
        if (chromeNowMissing) {
            if (rc == firstResponse) { chromeMissing = NO; first = YES; continue; }
            if (rc == firstResponse + 1) { open_with_chrome("https://www.google.com/chrome/"); continue; }
            return 0;
        }
        if (rc == firstResponse) continue;
        if (rc == firstResponse + 1) { open_with_chrome("chrome://extensions"); continue; }
        if (rc == firstResponse + 2) { reveal_extension_dir(extensionDir); continue; }
        if (rc == firstResponse + 3) { copy_extension_dir(extensionDir); continue; }
        return 0; // 取消：结束此次引导与检测；再次双击可继续。
    }
}
