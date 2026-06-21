#!/usr/bin/env bash
# ================================================================
# Natives2 Terminal Health Check
# 验证终端配置完整性：tauri 配置、capabilities、xterm 依赖、命令注册
# ================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PASS=0
FAIL=0
WARN=0

pass()  { echo "  ✅ $1"; PASS=$((PASS+1)); }
fail()  { echo "  ❌ $1"; FAIL=$((FAIL+1)); }
warn()  { echo "  ⚠️  $1"; WARN=$((WARN+1)); }

echo ""
echo "┌─────────────────────────────────────────────┐"
echo "│   Natives2 Terminal Health Check            │"
echo "└─────────────────────────────────────────────┘"
echo ""

# ── 1. tauri.conf.json 配置检查 ──
echo "■ tauri.conf.json"

TAURI_CONF="$ROOT_DIR/src-tauri/tauri.conf.json"
if [ -f "$TAURI_CONF" ]; then
  # frontendDist 应与 next.config.ts 的 distDir 一致
  FRONTEND_DIST=$(grep -o '"frontendDist"[[:space:]]*:[[:space:]]*"[^"]*"' "$TAURI_CONF" | cut -d'"' -f4)
  if [ "$FRONTEND_DIST" = "../out" ]; then
    pass "frontendDist=$FRONTEND_DIST 正确"
  else
    warn "frontendDist=$FRONTEND_DIST (预期 ../out)"
    WARN=$((WARN-1))
  fi

  # visible: false (FOUC 保护)
  if grep -q '"visible"[[:space:]]*:[[:space:]]*false' "$TAURI_CONF"; then
    pass "FOUC 保护: visible=false"
  else
    warn "FOUC 保护: visible 未设为 false"
  fi

  # 插件配置不导致 panic
  if grep -q '"plugins"' "$TAURI_CONF"; then
    pass "plugins 配置存在"
  fi
else
  fail "tauri.conf.json 不存在"
fi

# ── 2. next.config.ts ──
echo "■ next.config.ts"
NEXT_CONF="$ROOT_DIR/next.config.ts"
if [ -f "$NEXT_CONF" ]; then
  if grep -q "output.*export" "$NEXT_CONF"; then
    pass "output: 'export' — 静态导出"
  else
    fail "缺少 output: 'export'"
  fi
  if grep -q "distDir.*out" "$NEXT_CONF"; then
    pass "distDir: 'out' — 与 frontendDist 一致"
  else
    fail "distDir 非 'out'，与 frontendDist 不匹配"
  fi
else
  fail "next.config.ts 不存在"
fi

# ── 3. Rust 终端命令注册 ──
echo "■ Rust tauri command 注册"
LIB_RS="$ROOT_DIR/src-tauri/src/lib.rs"
if [ -f "$LIB_RS" ]; then
  for cmd in terminal_create terminal_write terminal_resize terminal_kill terminal_cwd terminal_proc terminal_session_state terminal_list_sessions; do
    if grep -q "$cmd" "$LIB_RS"; then
      pass "命令已注册: $cmd"
    else
      fail "命令未注册: $cmd"
    fi
  done
  for rec_cmd in terminal_record_start terminal_record_stop terminal_record_list terminal_record_play; do
    if grep -q "$rec_cmd" "$LIB_RS"; then
      pass "录制命令已注册: $rec_cmd"
    else
      fail "录制命令未注册: $rec_cmd"
    fi
  done
  if grep -q "ghostty_vt_available" "$LIB_RS"; then
    pass "ghostty_vt_available 已注册"
  fi
else
  fail "lib.rs 不存在"
fi

# ── 4. Rust 终端后端模块 ──
echo "■ Rust 后端 terminal.rs"
TERMINAL_RS="$ROOT_DIR/src-tauri/src/terminal.rs"
if [ -f "$TERMINAL_RS" ]; then
  if grep -q "SessionStatus" "$TERMINAL_RS"; then
    pass "SessionStatus 枚举已定义 (Running/Exiting/Exited)"
  fi
  if grep -q "SessionStateSnapshot" "$TERMINAL_RS"; then
    pass "SessionStateSnapshot 结构体已定义"
  fi
  if grep -q "session_counter.*AtomicU64" "$TERMINAL_RS"; then
    pass "session_counter 使用原子计数器（不复用 id）"
  fi
  if grep -q "fn parse_osc7" "$TERMINAL_RS"; then
    pass "OSC 7 (cwd) 解析器已实现"
  fi
  if grep -q "fn parse_osc_title" "$TERMINAL_RS"; then
    pass "OSC title 解析器已实现"
  fi
  if grep -q "fn proc" "$TERMINAL_RS"; then
    pass "proc() 前台进程检测已实现"
  fi
  if grep -q "_natives_chpwd" "$TERMINAL_RS"; then
    pass "Shell 钩子注入 (OSC 自动发送) 已实现"
  fi
else
  fail "terminal.rs 不存在"
fi

# ── 5. 录制模块 ──
echo "■ terminal_recorder.rs"
RECORDER_RS="$ROOT_DIR/src-tauri/src/terminal_recorder.rs"
if [ -f "$RECORDER_RS" ]; then
  if grep -q "fn stop" "$RECORDER_RS"; then
    pass "stop() 方法存在"
  fi
  if grep -q "flush" "$RECORDER_RS"; then
    pass "stop() 包含 flush"
  fi
  if grep -q "parse_cast_meta" "$RECORDER_RS"; then
    pass "list() 解析 header 获取真实 width/height/duration"
  fi
else
  fail "terminal_recorder.rs 不存在"
fi

# ── 6. 前端 Terminal.tsx ──
echo "■ 前端 Terminal.tsx"
TERMINAL_TSX="$ROOT_DIR/src/components/shell/Terminal.tsx"
if [ -f "$TERMINAL_TSX" ]; then
  if grep -q "ResizeObserver" "$TERMINAL_TSX"; then
    pass "ResizeObserver 已集成"
  fi
  if grep -q "realCols" "$TERMINAL_TSX"; then
    pass "先 mount xterm → fit → 再创建 PTY（真实 cols/rows）"
  fi
  if grep -q "terminal.*proc" "$TERMINAL_TSX"; then
    pass "Agent 启动使用 terminal.proc 检测前台进程"
  fi
  if grep -q "isRecording" "$TERMINAL_TSX"; then
    pass "录制按钮已添加"
  fi
  if grep -q "TerminalRecorder" "$TERMINAL_TSX"; then
    pass "TerminalRecorder 组件已引用"
  fi
  if grep -q "\\\\x0c" "$TERMINAL_TSX"; then
    pass "Cmd+K 清屏快捷键已添加"
  fi
  if grep -q "bracketed paste" "$TERMINAL_TSX"; then
    pass "bracketed paste 粘贴已添加"
  fi
else
  fail "Terminal.tsx 不存在"
fi

# ── 7. 前端 TerminalRecorder.tsx ──
echo "■ 前端 TerminalRecorder.tsx"
RECORDER_TSX="$ROOT_DIR/src/components/shell/TerminalRecorder.tsx"
if [ -f "$RECORDER_TSX" ]; then
  if grep -q "disableStdin.*true" "$RECORDER_TSX"; then
    pass "回放使用 xterm 只读实例 (disableStdin)"
  else
    warn "回放可能未使用 xterm 只读模式"
  fi
  if grep -q "from '@xterm/xterm'" "$RECORDER_TSX"; then
    pass "回放组件导入 @xterm/xterm"
  fi
else
  fail "TerminalRecorder.tsx 不存在"
fi

# ── 8. 前端 tauri-adapter ──
echo "■ tauri-adapter.ts"
ADAPTER_TS="$ROOT_DIR/src/lib/tauri-adapter.ts"
if [ -f "$ADAPTER_TS" ]; then
  for method in create write resize kill cwd proc sessionState listSessions; do
    if grep -q "$method" "$ADAPTER_TS"; then
      pass "adapter 方法: $method"
    else
      fail "adapter 缺少方法: $method"
    fi
  done
  if grep -q "ghosttyVtAvailable" "$ADAPTER_TS"; then
    pass "adapter: ghosttyVtAvailable 存在"
  fi
else
  fail "tauri-adapter.ts 不存在"
fi

# ── 9. 类型定义 ──
echo "■ 类型定义 types/index.ts"
TYPES_TS="$ROOT_DIR/src/types/index.ts"
if [ -f "$TYPES_TS" ]; then
  for field in proc sessionState listSessions; do
    if grep -q "$field" "$TYPES_TS"; then
      pass "类型定义: $field"
    else
      fail "类型定义缺少: $field"
    fi
  done
  if grep -q "ghosttyVtAvailable" "$TYPES_TS"; then
    pass "类型定义: ghosttyVtAvailable"
  fi
else
  fail "types/index.ts 不存在"
fi

# ── 10. Cargo 编译检查 ──
echo "■ Cargo check"
if cd "$ROOT_DIR/src-tauri" && cargo check 2>&1 | grep -q "Finished"; then
  pass "cargo check 通过"
else
  fail "cargo check 失败"
fi

# ── 11. TypeScript 编译检查 ──
echo "■ tsc --noEmit"
if cd "$ROOT_DIR" && npx tsc --noEmit 2>&1; then
  pass "tsc --noEmit 通过"
else
  fail "tsc --noEmit 存在类型错误"
fi

# ── 总结 ──
echo ""
echo "┌─────────────────────────────────────────────┐"
echo "│   检查完成                                  │"
echo "├─────────────────────────────────────────────┤"
printf "│   ✅ 通过: %-2d   ❌ 失败: %-2d   ⚠️  警告: %-2d    │\n" $PASS $FAIL $WARN
echo "└─────────────────────────────────────────────┘"
echo ""

# 退出码：有 FAIL 则返回 1
[ "$FAIL" -eq 0 ]
