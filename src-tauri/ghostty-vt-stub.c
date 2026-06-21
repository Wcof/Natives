/**
 * ghostty-vt-stub.c — 退化桩实现
 *
 * 当 Ghostty 源码不可用或构建环境不兼容时，提供 libghostty-vt.a 所需的全部符号。
 * 所有函数返回空/零值（不做实际 VT 解析），保证链接成功。
 * 替换为真正的 libghostty-vt.a 即可激活完整 VT 引擎。
 */
#include <stddef.h>
#include <stdint.h>

/* 返回码 */
#define GHOSTTY_SUCCESS 0
#define GHOSTTY_ERROR   (-1)

/* ── ghostty_terminal_new ── */
int ghostty_terminal_new(
    const void *allocator,
    void **terminal,
    const void *opts,
    size_t opts_len
) {
    (void)allocator;
    (void)terminal;
    (void)opts;
    (void)opts_len;
    return GHOSTTY_ERROR;
}

/* ── ghostty_terminal_free ── */
void ghostty_terminal_free(void *terminal) {
    (void)terminal;
}

/* ── ghostty_terminal_vt_write ── */
int ghostty_terminal_vt_write(
    void *terminal,
    const uint8_t *data,
    size_t len
) {
    (void)terminal;
    (void)data;
    (void)len;
    return GHOSTTY_ERROR;
}

/* ── ghostty_terminal_set ── */
int ghostty_terminal_set(
    void *terminal,
    unsigned int option,
    const void *value
) {
    (void)terminal;
    (void)option;
    (void)value;
    return GHOSTTY_ERROR;
}

/* ── ghostty_terminal_get ── */
int ghostty_terminal_get(
    void *terminal,
    unsigned int data_type,
    void *out
) {
    (void)terminal;
    (void)data_type;
    (void)out;
    return GHOSTTY_ERROR;
}

/* ── ghostty_terminal_free_string ── */
void ghostty_terminal_free_string(void *terminal, void *ptr) {
    (void)terminal;
    (void)ptr;
}
