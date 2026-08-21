#!/bin/bash
# ── AiNative Secret Disk/Log Scan（P0-A · ADR-0020 P0 Gate #6 · R-S12）──
# 扫描仓库工作区，确认不存在长期明文 Secret 落盘模式：
#   - 常见 API Key 前缀（sk-ant- / sk- / ghp_ / xoxb- 等）
#   - 明文 password/secret/token 赋值（非测试 fixture、非 mock）
#   - 日志/临时目录中的 Secret 回显
# 用法: bash scripts/secret-scan.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

PASS=true
COUNT=0

echo "═══════════════════════════════════════════════"
echo "  AiNative Secret Disk/Log Scan"
echo "═══════════════════════════════════════════════"

# 排除目录：构建产物、依赖、VCS、测试 fixture、spike 浏览器源码
EXCLUDES=(--exclude-dir=node_modules --exclude-dir=target --exclude-dir=.git \
  --exclude-dir=.next --exclude-dir=dist --exclude-dir=References \
  --exclude-dir=fixtures --exclude-dir=.runtime-evidence)
EXTRA_EXCLUDES=(--exclude='*.lock' --exclude='*.min.js' --exclude='*.map' \
  --exclude='Cargo.lock' --exclude='package-lock.json')

# 1. 高置信明文 Secret 模式（命中即失败）
# 测试模块中的"假密钥"是脱敏验证 fixture（断言 sanitize/redact 后不含密钥），
# 不是真实 Secret。策略：awk 逐文件状态机读取源码，跳过
# #[cfg(test)] / mod tests { ... } 测试块，仅在非测试代码中匹配 Secret 模式。
echo "1. 扫描高置信明文 Secret 模式（API Key / token 前缀）..."
SECRET_PATTERN='(sk-ant-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9]{32,}|ghp_[A-Za-z0-9]{36,}|xox[baprs]-[A-Za-z0-9-]{20,}|AIza[A-Za-z0-9_-]{35,}|-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----)'
HIGH_CONFIDENCE=$(find "$PROJECT_ROOT/src" "$PROJECT_ROOT/src-tauri/src" "$PROJECT_ROOT/src-agent-daemon/src" \
  "$PROJECT_ROOT/crates" "$PROJECT_ROOT/scripts" \
  \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.mjs' -o -name '*.js' \) \
  ! -name '*_tests.rs' ! -name '*_test.rs' ! -name '*.test.ts' ! -name '*.test.tsx' \
  ! -name '*.spec.ts' ! -name '*.spec.tsx' ! -name '*.test.js' ! -name '*.spec.js' \
  2>/dev/null | while IFS= read -r file; do
    awk -v file="$file" -v pattern="$SECRET_PATTERN" '
        {
            if (skip == 0) {
                # 进入测试块：#[cfg(test)] 或独立 mod tests
                if ($0 ~ /#\[cfg\(test\)\]/ || $0 ~ /^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{/) {
                    skip = 1; brace = 0
                    count_braces($0)
                    if (brace <= 0) { skip = 0 }
                    next
                }
                if (match($0, pattern) > 0) {
                    print file ":" NR ":" $0
                }
            } else {
                count_braces($0)
                if (brace <= 0) { skip = 0 }
            }
        }
        function count_braces(line,   n, i, c) {
            n = length(line)
            for (i = 1; i <= n; i++) {
                c = substr(line, i, 1)
                if (c == "{") brace++
                else if (c == "}") brace--
            }
        }' "$file"
  done)

if [ -n "$HIGH_CONFIDENCE" ]; then
  echo "  ❌ 发现高置信明文 Secret:"
  echo "$HIGH_CONFIDENCE"
  PASS=false
else
  echo "  ✅ 未发现高置信明文 Secret"
fi

# 2. 日志/临时目录中疑似 Secret 回显（仅告警，不阻断）
echo "2. 扫描日志/临时目录 Secret 回显（告警级）..."
LOG_ECHO=$(grep -rInE '(access_token|refresh_token|api[_-]?key|client_secret)["'"'"']?\s*[:=]\s*["'"'"'][A-Za-z0-9_-]{20,}' \
  "${EXCLUDES[@]}" "${EXTRA_EXCLUDES[@]}" \
  "$PROJECT_ROOT/src-tauri/src/log_sanitizer.rs" "$PROJECT_ROOT/src-tauri/src/secrets" 2>/dev/null || true)

if [ -n "$LOG_ECHO" ]; then
  echo "  ⚠️  疑似 Secret 回显（请人工确认是否经脱敏）:"
  echo "$LOG_ECHO"
else
  echo "  ✅ 未发现疑似 Secret 回显"
fi

# 3. SQLite 密文与主密钥同存检查（完成态禁忌，只报告现状）
echo "3. 检查 SQLite 同存密文/主密钥路径（迁移目标：Keychain）..."
SAME_STORE=$(grep -rIn 'provider_kek\|env_encryption_key' \
  "${EXCLUDES[@]}" "${EXTRA_EXCLUDES[@]}" \
  "$PROJECT_ROOT/src-tauri/src" 2>/dev/null || true)
echo "  ℹ️ 发现 $(echo "$SAME_STORE" | grep -c . || true) 处 KEK/密钥引用（迁移期现状，P0-A 已提供 Keychain 迁移语义，生产切换前必须迁出）"

echo ""
if [ "$PASS" = true ]; then
  echo "═══════════════════════════════════════════════"
  echo "  Secret Scan: PASS"
  echo "═══════════════════════════════════════════════"
  exit 0
else
  echo "═══════════════════════════════════════════════"
  echo "  Secret Scan: FAIL（存在明文 Secret，禁止合入）"
  echo "═══════════════════════════════════════════════"
  exit 1
fi
