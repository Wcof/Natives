#!/usr/bin/env node
// 边界依赖图门禁（mode/T08，A30～A33 / ADR-0039 R-M3/R-M5/R-M6/R-M12）。
//
// 覆盖声明的第一方生产入口：extension/**/*.js（递归；测试文件与 tests/
// 夹具按测试身份排除）、modules/<appId>/ui/src、modules/<appId>/src（Rust）。
// 规则：
//   G1 JS 导入图（静态 import、side-effect import、字面量动态 import()、
//      re-export、模板插值内实际执行的 import）不得成环；无法静态解析的
//      动态导入与「指向未纳入扫描文件的相对依赖」必须显式失败，不允许
//      从依赖图静默消失。
//   G2 Rust 领域文件（非 api/、非 app.rs/lib.rs、非 *_tests.rs 与
//      #[cfg(test)] 区域）不得引用 HTTP 适配层：直接（crate::api…）、
//      分组（use crate::{…api::…}）与别名（use crate::api as …）都检测。
//   G3 前端公开 action 唯一 owner：跨文件重复与同文件重复注册都违规；
//      覆盖 actions.NAME= 与 actions['NAME']=。
//   G4 声明入口必须真实存在；缺文件失败，不静默跳过。
//   G5 fund UI 不得直读 home 命名空间内部 state。
//
// 解析边界（诚实声明）：内置字符串感知扫描器——JS 按词法剥离注释、
// 字符串/字符字面量并按「模板字面量 = 字面量块 + ${} 插值代码」处理，
// 插值代码作为真实代码扫描（其中 import 是真实执行点）；静态正则字面量
// 内若含引号/反引号可能扰动分词——第一方入口避免此写法，门禁遇到无法
// 分词的文件会以解析失败显式报错而不是跳过。不做完整 AST；动态导入仅
// 当参数是单一字符串/无插值模板时可解析，其余一律 unresolved 失败。
// 脚本先运行内置正反例自检（P1～P23，含 2026-10-05 复核全部反例），
// 再扫描真实文件。
import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { join, dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const violations = [];
const appIds = JSON.parse(readFileSync(join(ROOT, 'modules/registry.json'), 'utf8')).apps.map(app => app.appId);

// ---------- 字符串感知扫描器 ----------

/// JS：注释删除；字符串/字符字面量与模板字面量块替换为 '§N' 占位；
/// 模板 ${…} 插值作为代码递归扫描。strings[N]={content,kind}，
/// kind ∈ string | template-plain | template-chunk。
export function maskLiterals(source, lang) {
  const strings = [];
  if (lang === 'js') return { code: jsMask(source, strings), strings };
  // Rust：//、/* */（可嵌套）、"..."、r"..."、r#"..."#、字符字面量；
  // 生命周期 'a 视作代码。
  let i = 0;
  const n = source.length;
  let out = '';
  while (i < n) {
    const ch = source[i];
    const nx = source[i + 1];
    if (ch === '/' && nx === '/') { while (i < n && source[i] !== '\n') i++; continue; }
    if (ch === '/' && nx === '*') {
      let depth = 1; i += 2;
      while (i < n && depth > 0) {
        if (source[i] === '/' && source[i + 1] === '*') { depth++; i += 2; continue; }
        if (source[i] === '*' && source[i + 1] === '/') { depth--; i += 2; continue; }
        i++;
      }
      continue;
    }
    if (ch === 'r' && (source[i + 1] === '"' || source[i + 1] === '#')) {
      let hashes = 0;
      let j = i + 1;
      while (source[j] === '#') { hashes++; j++; }
      if (source[j] === '"') {
        const terminator = '"' + '#'.repeat(hashes);
        i = source.indexOf(terminator, j + 1);
        i = i === -1 ? n : i + terminator.length;
        out += "''";
        continue;
      }
      out += ch; i++;
      continue;
    }
    if (ch === '"') {
      i++;
      while (i < n) {
        if (source[i] === '\\') { i += 2; continue; }
        if (source[i] === '"') break;
        i++;
      }
      i++;
      out += "''";
      continue;
    }
    if (ch === "'") {
      // 字符字面量 'x' / '\n'；生命周期标注 'a 保留为代码。
      if (source[i + 2] === "'" && source[i + 1] !== undefined) { i += 3; out += "''"; continue; }
      out += ch; i++;
      continue;
    }
    out += ch; i++;
  }
  return { code: out, strings };
}

/// 从占位符 token 取字符串元数据下标。
const placeholder = (token) => Number(String(token).replace(/\D/g, ''));

export function jsMask(source, strings) {
  const n = source.length;
  let i = 0;
  const emit = (content, kind) => {
    const index = strings.push({ content, kind }) - 1;
    return `'§${index}'`;
  };
  // 全部结果经返回值拼接（复合赋值会在 RHS 前快照左值，闭包追加会被丢弃）。
  // stopBrace=true 时遇到匹配的 '}' 返回（模板插值边界）。
  const scanCode = (stopBrace) => {
    let code = '';
    let braces = 0;
    while (i < n) {
      const ch = source[i];
      const nx = source[i + 1];
      if (ch === '/' && nx === '/') { while (i < n && source[i] !== '\n') i++; continue; }
      if (ch === '/' && nx === '*') { i += 2; while (i < n && !(source[i] === '*' && source[i + 1] === '/')) i++; i += 2; continue; }
      if (ch === "'" || ch === '"') {
        const quote = ch;
        let content = '';
        i++;
        while (i < n) {
          if (source[i] === '\\') { content += source[i] + (source[i + 1] ?? ''); i += 2; continue; }
          if (source[i] === quote) break;
          content += source[i];
          i++;
        }
        i++;
        code += emit(content, 'string');
        continue;
      }
      if (ch === '`') { i++; code += scanTemplate(); continue; }
      if (ch === '{') braces++;
      if (ch === '}' && stopBrace) {
        if (braces === 0) { i++; return code; }
        braces--;
      }
      code += ch; i++;
    }
    return code;
  };
  const scanTemplate = () => {
    let literal = '';
    let masked = '';
    let hasInterp = false;
    while (i < n) {
      if (source[i] === '\\') { literal += source[i] + (source[i + 1] ?? ''); i += 2; continue; }
      if (source[i] === '`') { i++; break; }
      if (source[i] === '$' && source[i + 1] === '{') {
        hasInterp = true; i += 2;
        if (literal) { masked += emit(literal, 'template-chunk'); literal = ''; }
        masked += '{' + scanCode(true) + '}';
        continue;
      }
      literal += source[i]; i++;
    }
    if (literal) masked += emit(literal, hasInterp ? 'template-chunk' : 'template-plain');
    return masked;
  };
  return scanCode(false);
}


// ---------- JS 导入图 ----------

export function analyzeJsGraph(files, root = ROOT) {
  const nodes = new Set(files.map((f) => f.path));
  const graph = new Map(files.map((f) => [f.path, new Set()]));
  const unresolved = [];
  for (const file of files) {
    const { code, strings } = maskLiterals(file.source, 'js');
    const deps = graph.get(file.path);
    const resolveImport = (raw) => {
      const target = raw.split('?')[0];
      if (!target.startsWith('.')) return null; // 第三方/裸包名/内置模块
      const base = resolve(dirname(file.abs), target);
      const rel = relative(root, base).split('\\').join('/');
      if (nodes.has(rel)) return { node: rel };
      return { missing: rel };
    };
    const record = (resolved) => {
      if (!resolved) return;
      if (resolved.node) deps.add(resolved.node);
      else unresolved.push({ file: file.path, kind: 'missing', target: resolved.missing });
    };
    // 静态 import/export … from '…'（模块说明符只能是字符串字面量）。
    for (const m of code.matchAll(/(?:^|[\s;}{])(?:import|export)\s[\s\S]*?from\s*'§(\d+)'/g)) {
      const meta = strings[placeholder(m[1])];
      if (meta && meta.kind === 'string') record(resolveImport(meta.content));
    }
    // side-effect import '…'。
    for (const m of code.matchAll(/(?:^|[\s;}{])import\s*'§(\d+)'/g)) {
      const meta = strings[placeholder(m[1])];
      if (meta && meta.kind === 'string') record(resolveImport(meta.content));
    }
    // 动态 import(…)：参数是单一字符串/无插值模板 → 解析；其余 unresolved。
    for (const m of code.matchAll(/(?:^|[^\w$.])import\s*\(/g)) {
      let depth = 1;
      let j = m.index + m[0].length;
      const argStart = j;
      while (j < code.length && depth > 0) {
        if (code[j] === '(') depth++;
        else if (code[j] === ')') depth--;
        j++;
      }
      const arg = code.slice(argStart, j - 1).trim();
      const single = /^'§(\d+)'$/.exec(arg);
      if (single) {
        const meta = strings[placeholder(single[1])];
        if (meta && (meta.kind === 'string' || meta.kind === 'template-plain')) {
          record(resolveImport(meta.content));
          continue;
        }
      }
      const readable = arg.replace(/'§(\d+)'/g, (_, k) => JSON.stringify(strings[Number(k)]?.content ?? '?'));
      unresolved.push({ file: file.path, kind: 'computed', expr: readable });
    }
  }
  const cycles = [];
  const color = new Map();
  const stack = [];
  const dfs = (node) => {
    color.set(node, 1);
    stack.push(node);
    for (const next of [...(graph.get(node) ?? [])].sort()) {
      if (color.get(next) === 1) cycles.push([...stack.slice(stack.indexOf(next)), next]);
      else if (!color.get(next)) dfs(next);
    }
    stack.pop();
    color.set(node, 2);
  };
  for (const name of [...nodes].sort()) if (!color.get(name)) dfs(name);
  return { cycles, unresolved };
}

// ---------- Rust 反向依赖 ----------

export function rustReverseDeps(files) {
  const hits = [];
  for (const file of files) {
    if (file.rel.endsWith('_tests.rs')) continue; // 测试文件是调用 HTTP 入口的合法身份
    const { code } = maskLiterals(file.source, 'rust');
    // 仅移除测试 item，不能把其后的生产代码一起排除。
    let production = code;
    for (let start; (start = production.indexOf('#[cfg(test)]')) !== -1;) {
      let end = start + '#[cfg(test)]'.length;
      while (end < production.length && production[end] !== '{' && production[end] !== ';') end++;
      if (production[end] === '{') {
        let depth = 1;
        while (++end < production.length && depth) {
          if (production[end] === '{') depth++;
          if (production[end] === '}') depth--;
        }
      }
      production = production.slice(0, start) + ' ' + production.slice(end + 1);
    }
    if (file.rel.startsWith('api/') || file.rel === 'app.rs' || file.rel === 'lib.rs') continue;
    // 语句级检测：直接路径、分组 use（任意嵌套层级内出现 api 段）与别名。
    if (/\bcrate\s*::\s*api\b/.test(production)) {
      hits.push({ rel: file.rel });
      continue;
    }
    for (const stmt of production.matchAll(/\buse\s+[^;]+;/g)) {
      const text = stmt[0];
      const direct = /crate::api\b/.test(text);
      const grouped = /crate::\{/.test(text) && /\bapi\s*(?:::|as\b|[,}])/.test(text);
      if (direct || grouped) {
        hits.push({ rel: file.rel });
        break;
      }
    }
  }
  return hits;
}

// ---------- action owner ----------

export function duplicateActionOwners(files) {
  const owners = new Map();
  for (const file of files) {
    const { code, strings } = maskLiterals(file.source, 'js');
    const register = (action) => {
      if (!owners.has(action)) owners.set(action, new Map());
      const perFile = owners.get(action);
      perFile.set(file.path, (perFile.get(file.path) ?? 0) + 1);
    };
    for (const m of code.matchAll(/\bactions\.(\w+)\s*=(?!=)/g)) register(m[1]);
    for (const m of code.matchAll(/\bactions\[\s*'§(\d+)'\s*\]\s*=(?!=)/g)) register(strings[placeholder(m[1])].content);
  }
  const result = [];
  for (const [action, perFile] of owners) {
    const total = [...perFile.values()].reduce((a, b) => a + b, 0);
    if (perFile.size > 1) {
      result.push({ action, files: [...perFile.keys()].sort(), registrations: total, kind: 'cross-file' });
    } else if (total > 1) {
      result.push({ action, files: [...perFile.keys()], registrations: total, kind: 'same-file' });
    }
  }
  return result.sort((a, b) => a.action.localeCompare(b.action));
}

// ---------- 自检（P1～P23：既有正反例 + 2026-10-05 复核反例） ----------

function assertSelfTests() {
  const check = (cond, msg) => { if (!cond) throw new Error(msg); };
  const A = (files) => analyzeJsGraph(files, '/x');

  // P1：JS 普通字符串不得成为导入边。
  const p1 = A([
    { path: 'a.js', abs: '/x/a.js', source: "const s = \"import('./b.js')\"; const t = 'import(\\'./b.js\\')'; // import('./b.js')\n" },
    { path: 'b.js', abs: '/x/b.js', source: 'export const b = 1;\n' },
  ]);
  check(p1.cycles.length === 0, 'P1 失败：普通字符串被当成导入边');
  // P2：side-effect import 环必须检测。
  const p2 = A([
    { path: 'a.js', abs: '/x/a.js', source: "import './b.js';\n" },
    { path: 'b.js', abs: '/x/b.js', source: "import './a.js';\n" },
  ]);
  check(p2.cycles.length === 1, 'P2 失败：side-effect import 环漏检');
  // P3：../ 相对父目录导入环必须检测。
  const p3 = A([
    { path: 'mod/ui/src/a.js', abs: '/x/mod/ui/src/a.js', source: "import { b } from '../../src/b.js';\n" },
    { path: 'mod/src/b.js', abs: '/x/mod/src/b.js', source: "import { a } from '../ui/src/a.js';\n" },
  ]);
  check(p3.cycles.length === 1, 'P3 失败：../ 相对导入环漏检');
  // P4：Rust grouped use 反向依赖必须检测。
  const p4 = rustReverseDeps([{ rel: 'cards.rs', source: 'use crate::api::{json_str, today_iso};\n' }]);
  check(p4.length === 1, 'P4 失败：Rust grouped use 反向依赖漏检');
  // P5：Rust 普通字符串不得成为反向依赖。
  const p5 = rustReverseDeps([{ rel: 'home.rs', source: 'let hint = "参照 crate::api::json_str 的口径"; // use crate::api::json_str;\n' }]);
  check(p5.length === 0, 'P5 失败：Rust 普通字符串/注释误报');
  // P6：bracket action 重复注册必须检测。
  const p6 = duplicateActionOwners([
    { path: 'a.js', source: "actions['openThing'] = () => {};\n" },
    { path: 'b.js', source: 'actions.openThing = () => {};\n' },
  ]);
  check(p6.length === 1 && p6[0].action === 'openThing', 'P6 失败：bracket action 重复注册漏检');
  // P7：同名文件按路径身份区分。
  const p7 = A([
    { path: 'extension/app.js', abs: '/x/extension/app.js', source: "import { shell } from './app-support.js';\n" },
    { path: 'extension/app-support.js', abs: '/x/extension/app-support.js', source: "import { shell } from './app.js';\n" },
    { path: 'modules/fund/ui/src/app.js', abs: '/x/modules/fund/ui/src/app.js', source: 'export const unrelated = 1;\n' },
    { path: 'modules/fund/ui/src/app-support.js', abs: '/x/modules/fund/ui/src/app-support.js', source: 'export const unrelated = 2;\n' },
  ]);
  check(p7.cycles.length === 1, 'P7 失败：同名文件路径身份混淆');
  // P8：合法字符串/注释中的 actions 记号不误报。
  const p8 = duplicateActionOwners([{ path: 'a.js', source: 'const s = "actions.openThing = fake"; // actions.openThing =\n' }]);
  check(p8.length === 0, 'P8 失败：字符串/注释中的 actions 记号误报');
  // ==== 2026-10-05 复核反例 ====
  // P9：import 与括号之间的空格仍须解析为动态导入并检出环。
  const p9 = A([
    { path: 'a.js', abs: '/x/a.js', source: "const m = import ('./b.js');\n" },
    { path: 'b.js', abs: '/x/b.js', source: "import './a.js';\n" },
  ]);
  check(p9.cycles.length === 1, 'P9 失败：import( 空格形式漏检');
  // P10：模板插值内实际执行的 import 必须产生边。
  const p10 = A([
    { path: 'a.js', abs: '/x/a.js', source: "const s = `${import('./b.js')}`;\n" },
    { path: 'b.js', abs: '/x/b.js', source: "import './a.js';\n" },
  ]);
  check(p10.cycles.length === 1, 'P10 失败：模板插值内 import 漏检');
  // P11：计算式模板导入必须 unresolved，不得静默接受。
  const p11 = A([
    { path: 'a.js', abs: '/x/a.js', source: "const name = './b.js'; const m = import(`${name}`);\n" },
    { path: 'b.js', abs: '/x/b.js', source: 'export const b = 1;\n' },
  ]);
  check(p11.unresolved.length === 1 && p11.unresolved[0].kind === 'computed', 'P11 失败：计算式动态导入被静默忽略');
  // P12：指向未纳入扫描文件的相对依赖必须显式失败。
  const p12 = A([{ path: 'a.js', abs: '/x/a.js', source: "import './missing.js';\n" }]);
  check(p12.unresolved.length === 1 && p12.unresolved[0].kind === 'missing', 'P12 失败：缺失相对依赖被静默忽略');
  // P13：Rust 嵌套分组 use 中的 api 必须识别。
  const p13 = rustReverseDeps([{ rel: 'cards.rs', source: 'use crate::{api::json_str, storage::Store};\n' }]);
  check(p13.length === 1, 'P13 失败：Rust 嵌套分组 use 漏检');
  // P14：Rust 别名引用必须识别。
  const p14 = rustReverseDeps([{ rel: 'cards.rs', source: 'use crate::api as http;\n' }]);
  check(p14.length === 1, 'P14 失败：Rust 别名引用漏检');
  // P15：同一文件重复注册同一 action 必须检测。
  const p15 = duplicateActionOwners([{ path: 'a.js', source: 'actions.open = first; actions.open = second;\n' }]);
  check(p15.length === 1 && p15[0].kind === 'same-file' && p15[0].registrations === 2, 'P15 失败：同文件重复注册漏检');
  // P16：无插值模板导入可解析并检出环。
  const p16 = A([
    { path: 'a.js', abs: '/x/a.js', source: "const m = import(`./b.js`);\n" },
    { path: 'b.js', abs: '/x/b.js', source: "import './a.js';\n" },
  ]);
  check(p16.cycles.length === 1 && p16.unresolved.length === 0, 'P16 失败：无插值模板导入处理错误');
  // P17：合法代码不误报——跨文件正常导入、模板插值内无 import、单一注册。
  const p17 = A([
    { path: 'a.js', abs: '/x/a.js', source: "import { b } from './b.js'; const label = `btn ${1 + 1}`;\n" },
    { path: 'b.js', abs: '/x/b.js', source: 'export const b = 1;\n' },
  ]);
  check(p17.cycles.length === 0 && p17.unresolved.length === 0, 'P17 失败：合法模板插值/导入误报');
  const p18 = duplicateActionOwners([{ path: 'a.js', source: 'actions.open = () => {};\n' }]);
  check(p18.length === 0, 'P18 失败：单一注册误报');

  for (const source of ["const m=import('./b.js');", "const s = `${({x:1}) && import('./b.js')}`;"]) {
    check(A([{path:'a.js',abs:'/x/a.js',source},{path:'b.js',abs:'/x/b.js',source:"import './a.js';"}]).cycles.length === 1, 'P19：紧邻运算符／嵌套模板导入漏检');
  }
  check(rustReverseDeps([{rel:'domain.rs',source:'fn run() { crate::api::handler(); }'}]).length === 1, 'P20：函数内限定路径漏检');

  check(rustReverseDeps([{rel:'domain.rs',source:'#[cfg(test)] mod tests { fn t() {} } fn production() { crate::api::handler(); }'}]).length === 1, 'P21：测试 item 后的生产依赖漏检');
  check(rustReverseDeps([{rel:'domain.rs',source:'use crate::{api, storage::Store};'}]).length === 1, 'P22：分组模块导入漏检');
  check(rustReverseDeps([{rel:'domain.rs',source:'#[cfg(test)] mod tests { fn t() { crate::api::handler(); } } fn production() {}'}]).length === 0, 'P23：合法测试引用误报');
  console.log('边界图门禁自检通过（P1～P23）');
}

// ---------- 入口集合 ----------

function listFiles(dir, ext, out = []) {
  if (!existsSync(dir)) return out;
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, ent.name);
    if (ent.isDirectory()) listFiles(full, ext, out);
    else if (ent.name.endsWith(ext)) out.push(full);
  }
  return out;
}

function requireEntries(entries) {
  for (const entry of entries) {
    const full = join(ROOT, entry);
    const ok = existsSync(full) && (statSync(full).isFile() || statSync(full).isDirectory());
    if (!ok) violations.push(`声明的第一方入口缺失：${entry}（G4：缺入口必须失败，不允许静默跳过）`);
  }
}

const JS_SKIP = new Set(['echarts.min.js']);
const isTestFile = (absPath) => {
  const rel = relative(ROOT, absPath).split('\\').join('/');
  return rel.endsWith('.test.mjs') || rel.includes('/tests/') || rel.endsWith('/test-dom-mock.js') || rel.endsWith('/ui-harness.js');
};

/// 第一方 JS：extension/**/*.js（递归；测试身份排除）+ modules/*/ui/src。
function firstPartyJsFiles() {
  const files = [];
  for (const full of listFiles(join(ROOT, 'extension'), '.js')) {
    if (isTestFile(full)) continue;
    if (JS_SKIP.has(full.split('\\').join('/').split('/').pop())) continue;
    files.push({ path: relative(ROOT, full).split('\\').join('/'), abs: full, source: readFileSync(full, 'utf8') });
  }
  for (const appId of appIds) {
    for (const full of listFiles(join(ROOT, 'modules', appId, 'ui', 'src'), '.js')) {
      const name = full.split('\\').join('/').split('/').pop();
      if (JS_SKIP.has(name)) continue;
      files.push({ path: relative(ROOT, full).split('\\').join('/'), abs: full, source: readFileSync(full, 'utf8') });
    }
  }
  return files;
}

function moduleRustFiles(appId) {
  const dir = join(ROOT, 'modules', appId, 'src');
  if (!existsSync(dir)) return [];
  return listFiles(dir, '.rs').map((full) => ({
    rel: relative(join(ROOT, 'modules', appId, 'src'), full).split('\\').join('/'),
    source: readFileSync(full, 'utf8'),
  }));
}

assertSelfTests();

requireEntries([
  'extension/app.js', 'extension/app-boot.js', 'extension/apps.js', 'extension/apps-boot.js',
  'extension/model-settings.js', 'extension/module-inference-broker.js',
  'extension/ai-performance/client.js', 'extension/ai-usage-client/client.js',
  'extension/plugins/sanitizer.js',
  'modules/fund/ui/src/app.js', 'modules/fund/ui/src/home.js',
  'modules/tokenusage/ui/src', 'modules/fund/src/api/mod.rs', 'modules/fund/src/app.rs',
  'modules/fund/src/watchlists.rs', 'modules/fund/src/chart_drawings.rs',
  'modules/fund/src/notification_send.rs',
  'crates/app-runtime/src/main.rs', 'crates/app-runtime-core/src/module.rs',
  'scripts/apps/check-module-boundaries.mjs', 'scripts/apps/check-core-business-leaks.mjs',
  'scripts/apps/check-module-contract.mjs',
]);

// G1：JS 导入图（递归范围；missing/computed 显式失败）。
const allJs = firstPartyJsFiles();
const graph = analyzeJsGraph(allJs);
for (const cycle of graph.cycles) violations.push(`JS 导入环（G1）：${cycle.join(' -> ')}`);
for (const item of graph.unresolved) {
  if (item.kind === 'missing') violations.push(`相对依赖指向未纳入扫描的文件（G1）：${item.file} -> ${item.target}`);
  else violations.push(`无法静态解析的动态导入（G1）：${item.file} import(${item.expr})`);
}

// G2：Rust 领域反向依赖（直接/分组/别名；字符串与注释免疫；测试区域排除）。
for (const appId of appIds) {
  for (const hit of rustReverseDeps(moduleRustFiles(appId))) {
    violations.push(`Rust 领域文件反向引用 HTTP 适配层（G2）：modules/${appId}/src/${hit.rel}`);
  }
}

// G3：action 唯一 owner（fund UI；dot+bracket；跨文件与同文件重复）。
const moduleUi = allJs.filter((f) => f.path.startsWith('modules/fund/ui/src/'));
for (const dup of duplicateActionOwners(moduleUi)) {
  if (dup.kind === 'same-file') violations.push(`公开 action 同文件重复注册（G3）：actions.${dup.action} 在 ${dup.files.join(', ')} 注册 ${dup.registrations} 次`);
  else violations.push(`公开 action 重复 owner（G3）：actions.${dup.action} 在 ${dup.files.join(', ')}`);
}

// G5：fund UI 不得直读 home 命名空间内部 state（T05 收口固化）。
// 覆盖直接形式（home.state）与别名形式（const h=home(); h.state.displayMode
// ——displayMode/privacy/snapshot 三个属性名在 home 命名空间上唯一，业务
// 记录的 state 字段（tx.state/v.state/p.state）不会命中）。
const HOME_STATE_PROPS = /\b\w+\.state\.(displayMode|privacy|snapshot)\b|\bhome\.state\b/;
for (const file of moduleUi) {
  const { code } = maskLiterals(file.source, 'js');
  const m = HOME_STATE_PROPS.exec(code);
  if (m) {
    violations.push(`fund UI 直读 home 内部 state（G5，R-M5）：${file.path} 匹配 ${m[0]}（经 home.displayMode()/privacy()/snapshot() 访问）`);
  }
}

if (violations.length) {
  console.error('边界依赖图门禁失败：');
  for (const v of violations) console.error(`  - ${v}`);
  process.exit(1);
}
console.log(`边界依赖图门禁通过：JS 图 ${allJs.length} 个生产文件（递归范围；side-effect/动态/模板插值/re-export 扫描；missing/computed 显式失败）无环；注册模块 Rust 领域反向依赖 0（直接/分组/别名）；fund action 唯一 owner（dot+bracket+同文件重复）成立；home.state 直读 0`);
