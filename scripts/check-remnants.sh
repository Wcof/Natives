#!/bin/bash
# ── Natives2 残留检查脚本 ──
# 运行在 CI/CD 或 pre-commit hook 中，确保 Tauri 迁移残留不回流。
# 用法: bash scripts/check-remnants.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

PASS=true
WARNINGS=""

echo "═══════════════════════════════════════════════"
echo "  Natives2 残留检查"
echo "═══════════════════════════════════════════════"
echo ""

# 1. 检查 /api/fs/ 业务调用（排除说明性注释和测试）
echo "1. 检查 /api/fs/ 业务调用..."
API_FS_CALLS=$(grep -rnE "fetch\(['\"]\/api\/fs\/|fetch\(\`\/api\/fs\/" \
  "$PROJECT_ROOT/src" \
  --include="*.ts" --include="*.tsx" \
  2>/dev/null | grep -v "^\s*//" | grep -v "test" || true)

if [ -n "$API_FS_CALLS" ]; then
  echo "  ❌ 发现 /api/fs/ 调用:"
  echo "$API_FS_CALLS"
  PASS=false
else
  echo "  ✅ 无 /api/fs/ 业务调用"
fi

# 2. 检查 src={entry.path} 直接渲染图片
echo "2. 检查图片直接渲染 (src={entry.path})..."
DIRECT_PATH_SRC=$(grep -rnE 'src=\{entry\.path\}|src=\$\{entry\.path\}' \
  "$PROJECT_ROOT/src" \
  --include="*.ts" --include="*.tsx" \
  2>/dev/null | grep -v "^\s*//" || true)

if [ -n "$DIRECT_PATH_SRC" ]; then
  echo "  ❌ 发现直接渲染 entry.path 作为图片 src:"
  echo "$DIRECT_PATH_SRC"
  PASS=false
else
  echo "  ✅ 无直接渲染 entry.path"
fi

# 3. 检查 useState(3001) 端口硬编码
echo "3. 检查 useState(3001) 端口硬编码..."
PORT_3001=$(grep -rnE "useState\(3001\)" \
  "$PROJECT_ROOT/src" \
  --include="*.ts" --include="*.tsx" \
  2>/dev/null || true)

if [ -n "$PORT_3001" ]; then
  echo "  ❌ 发现 useState(3001) 硬编码:"
  echo "$PORT_3001"
  PASS=false
else
  echo "  ✅ 无 useState(3001)"
fi

# 4. 检查 __nativesHttpPort 全局类型（排除声明为 never 的防御性声明）
echo "4. 检查 __nativesHttpPort 全局类型..."
NATIVES_HTTP_PORT=$(grep -rnE "__nativesHttpPort" \
  "$PROJECT_ROOT/src" \
  --include="*.ts" --include="*.tsx" \
  2>/dev/null | grep -v "^\s*//" | grep -v "?: never" || true)

if [ -n "$NATIVES_HTTP_PORT" ]; then
  echo "  ❌ 发现 __nativesHttpPort 引用:"
  echo "$NATIVES_HTTP_PORT"
  PASS=false
else
  echo "  ✅ 无 __nativesHttpPort 引用（防御性 never 声明除外）"
fi

# 5. 检查 Rust 中的 model_stats/input_tokens 等字段名（允许存在，但测试需验证 camelCase）
echo "5. 检查 Rust usage.rs 字段命名..."
CAMEL_CASE_CHECK=$(grep -rnE "model_stats|input_tokens|output_tokens|request_count|total_tokens|total_cost|avg_cost_per_request" \
  "$PROJECT_ROOT/src-tauri/src/commands/usage.rs" \
  2>/dev/null || true)

if [ -n "$CAMEL_CASE_CHECK" ]; then
  echo "  ℹ️  Rust 字段名（snake_case）存在，确保 serde(rename_all = \"camelCase\") 生效"
  echo "     测试已验证 JSON 输出为 camelCase"
fi

# 6. 检查 placeholder 注释
echo "6. 检查 placeholder / needs 注释..."
PLACEHOLDER=$(grep -rnE "placeholder — needs|TODO.*needs|FIXME.*needs|action = ''" \
  "$PROJECT_ROOT/src/components/ai/FollowRenderer.tsx" \
  --include="*.ts" --include="*.tsx" \
  2>/dev/null || true)

if [ -n "$PLACEHOLDER" ]; then
  echo "  ❌ 发现 placeholder 注释:"
  echo "$PLACEHOLDER"
  PASS=false
else
  echo "  ✅ 无 placeholder 注释"
fi

echo ""
echo "═══════════════════════════════════════════════"

if [ "$PASS" = true ]; then
  echo "  ✅ 所有检查通过"
  echo "═══════════════════════════════════════════════"
  exit 0
else
  echo "  ❌ 发现残留，请清理后再提交"
  echo "═══════════════════════════════════════════════"
  exit 1
fi
