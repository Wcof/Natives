/* Natives 主入口包装（实施方案 §1.1/§3.3）：Finder 双击 Natives.app 时
 * 由 LaunchServices 启动本可执行文件。它只做一件事——调用同 bundle 内的
 * native-file-host 引导模式；退出码 2（setup_required）时转入首次引导。
 * 不弹 Terminal（纯 Mach-O，无 shell 脚本），不承载业务 UI。 */
#include <libgen.h>
#include <limits.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

static int run_host(const char *host, const char *mode, const char *a1, const char *a2) {
    const char *child_argv[5] = {host, mode, a1, a2, NULL};
    pid_t pid = 0;
    posix_spawnattr_t attr;
    posix_spawnattr_init(&attr);
    int status = -1;
    if (posix_spawn(&pid, host, NULL, &attr, (char *const *)child_argv, environ) != 0) {
        return 127;
    }
    posix_spawnattr_destroy(&attr);
    while (waitpid(pid, &status, 0) < 0) {
    }
    if (WIFEXITED(status)) return WEXITSTATUS(status);
    return -1;
}

int main(int argc, char **argv) {
    (void)argc;
    char self[PATH_MAX];
    if (!realpath(argv[0], self)) {
        return 1;
    }
    char dir[PATH_MAX];
    strncpy(dir, dirname(self), sizeof(dir) - 1);
    dir[sizeof(dir) - 1] = '\0';
    char host[PATH_MAX];
    snprintf(host, sizeof(host), "%s/native-file-host", dir);
    int code = run_host(host, "--launcher-default", "", "");
    if (code == 2) {
        code = run_host(host, "--launcher-setup", "--setup-timeout-secs", "600");
    }
    return code == 2 ? 0 : code;
}
