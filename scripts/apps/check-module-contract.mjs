#!/usr/bin/env node
// 模块契约一致性门禁（mode/T01 / ADR-0039 R-M9 / 技术 07 v2.0.0）。
//
// 检查层次（完整签名级，非名称级）：
//   C1 trait 方法完整签名：receiver（&self/&mut self）、参数类型、返回类型
//      在生产（crates/app-runtime-core/src/module.rs）与技术 07 §3 代码块
//      之间逐字符一致（空白/换行归一后比较；仅格式、注释变化不误报）。
//   C2 trait 归属：每个方法在两侧属于同一 trait。
//   C3 ModuleContext 字段类型：生产声明与 pinned 契约表（本文件内，类型
//      变化必须显式更新表格与技术 07）逐字段一致。
//   C4 card_snapshot 默认 501 语义：两侧默认实现均含 APP_CARD_SNAPSHOT_UNSUPPORTED。
//   C5 工厂对齐：registry 工厂 <crate>::<Type>::new 的类型必须在该模块
//      源码内实现 BuiltInAppModule。
//   C6 托管契约 §14 声明 start() 返回 Arc<dyn ModuleService> 且业务 HTTP
//      不在生命周期锁内。
//
// 解析边界（诚实声明）：基于结构化行匹配与括号配对的 trait 块提取，
// 不做完整 Rust AST 解析。签名以「去掉全部空白」的归一形态比较——等价
// 重排（换行、缩进、注释插入）不误报；任何 receiver/参数/返回类型差异
// 都会失败。脚本先运行内置漂移正反例自检（含审计反例：&ModuleContext→
// ModuleContext 必须失败），再扫描真实文件。
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const violations = [];
const fail = (msg) => violations.push(msg);

// ---------- 纯分析函数（可独立自检） ----------

// 已由 use 声明确立的两处类型路径缩写（生产全称 ⇄ 文档缩写为同一类型）；
// 这是路径解析，不是放宽签名比较的白名单。
const TYPE_PATH_EQUIVALENCES = [
  [/crate::protocol::CardSnapshotResult/g, 'CardSnapshotResult'],
  [/std::sync::Arc/g, 'Arc'],
];

export function normalize(source) {
  let out = source
    .replace(/\/\/[^\n]*/g, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\s+/g, '');
  for (const [pattern, replacement] of TYPE_PATH_EQUIVALENCES) out = out.replace(pattern, replacement);
  return out;
}

/// 提取 trait 块：{ traitName: [{ sig: 归一签名(含 receiver/参数/返回), receiver }] }
export function extractTraitMethods(source) {
  const traits = {};
  const traitRe = /(?:pub\s+)?trait\s+(\w+)[^{]*\{/g;
  let match;
  while ((match = traitRe.exec(source)) !== null) {
    const name = match[1];
    const bodyStart = match.index + match[0].length;
    // 括号配对取 trait 块体
    let depth = 1, i = bodyStart;
    while (i < source.length && depth > 0) {
      if (source[i] === '{') depth++;
      else if (source[i] === '}') depth--;
      i++;
    }
    const body = source.slice(bodyStart, i - 1);
    const methods = {};
    const methodRe = /fn\s+(\w+)\s*\(([^)]*)\)\s*(?:->\s*([^;{]+))?/g;
    for (const m of body.matchAll(methodRe)) {
      const [, mname, params, ret] = m;
      const receiver = /\b&mut\s*self\b/.test(params) ? '&mut self' : (/\b&\s*self\b|&self/.test(params) ? '&self' : 'none');
      // 尾随逗号（多行参数排版）与空白归一，等价格式不误报。
      const paramsNorm = normalize(params).replace(/,(\)|$)/, '$1');
      const sig = normalize(`fn ${mname}(${paramsNorm})${ret ? `-> ${ret}` : ''}`);
      methods[mname] = { sig, receiver };
    }
    traits[name] = methods;
  }
  return traits;
}

/// 提取结构体字段声明：{ field: 归一类型 }
export function extractStructFields(source, structName) {
  const structRe = new RegExp(`pub struct ${structName}[^{]*\\{`);
  const m = structRe.exec(source);
  if (!m) return null;
  const bodyStart = m.index + m[0].length;
  let depth = 1, i = bodyStart;
  while (i < source.length && depth > 0) {
    if (source[i] === '{') depth++;
    else if (source[i] === '}') depth--;
    i++;
  }
  const body = source.slice(bodyStart, i - 1).replace(/\/\/[^\n]*/g, '');
  const fields = {};
  for (const fm of body.matchAll(/pub\s+(\w+)\s*:\s*([^,\n]+)/g)) {
    fields[fm[1]] = normalize(fm[2]).replace(/,$/, '');
  }
  return fields;
}

/// C1/C2：比较两侧 trait 方法（签名+归属）。返回违规列表。
export function compareTraits(production, documented) {
  const problems = [];
  for (const [traitName, methods] of Object.entries(production)) {
    const docTrait = Object.entries(documented).find(([n]) => n === traitName);
    if (!docTrait) {
      problems.push(`文档缺少 trait ${traitName}（生产已声明）`);
      continue;
    }
    for (const [mname, meta] of Object.entries(methods)) {
      const docMethod = docTrait[1][mname];
      if (!docMethod) {
        problems.push(`文档 trait ${traitName} 缺少方法 ${mname}（生产签名 ${meta.sig}）`);
        continue;
      }
      if (docMethod.sig !== meta.sig) {
        problems.push(`签名漂移（${traitName}.${mname}）：生产 ${meta.sig} vs 文档 ${docMethod.sig}`);
      }
      if (docMethod.receiver !== meta.receiver) {
        problems.push(`receiver 漂移（${traitName}.${mname}）：生产 ${meta.receiver} vs 文档 ${docMethod.receiver}`);
      }
    }
  }
  for (const [traitName, methods] of Object.entries(documented)) {
    if (!production[traitName]) problems.push(`文档多出 trait ${traitName}（生产无此声明）`);
    else {
      for (const mname of Object.keys(methods)) {
        if (!production[traitName][mname]) problems.push(`文档 trait ${traitName} 多出方法 ${mname}（生产未声明）`);
      }
    }
  }
  return problems;
}

function read(rel) {
  const full = join(ROOT, rel);
  if (!existsSync(full)) {
    fail(`缺失权威文件：${rel}`);
    return '';
  }
  return readFileSync(full, 'utf8');
}

function assertSelfTests() {
  // 漂移正例（审计反例）：initialize 参数 &ModuleContext → ModuleContext 必须失败。
  const prod = 'pub trait BuiltInAppModule { fn initialize(&mut self, context: &ModuleContext) -> Result<(), String>; fn start(&mut self) -> Result<std::sync::Arc<dyn ModuleService>, String>; }';
  const drifted = 'pub trait BuiltInAppModule { fn initialize(&mut self, context: ModuleContext) -> Result<(), String>; }';
  let problems = compareTraits(extractTraitMethods(prod), extractTraitMethods(drifted));
  if (!problems.some((p) => p.includes('签名漂移'))) throw new Error('C1 自检失败：参数类型漂移未被识别（审计反例）');
  // 漂移正例：返回类型变化必须失败。
  const driftedRet = 'pub trait BuiltInAppModule { fn initialize(&mut self, context: &ModuleContext) -> Result<bool, String>; }';
  problems = compareTraits(extractTraitMethods(prod), extractTraitMethods(driftedRet));
  if (!problems.some((p) => p.includes('签名漂移'))) throw new Error('C1 自检失败：返回类型漂移未被识别');
  // 漂移正例：receiver 变化必须失败。
  const driftedRecv = 'pub trait BuiltInAppModule { fn initialize(&mut self, context: &ModuleContext) -> Result<(), String>; }'.replace('fn initialize(&mut self', 'fn initialize(&self');
  problems = compareTraits(extractTraitMethods(prod), extractTraitMethods(driftedRecv));
  if (!problems.some((p) => p.includes('receiver 漂移'))) throw new Error('C1 自检失败：receiver 漂移未被识别');
  // 合法反例：等价换行/缩进/注释不得误报。
  const reformatted = 'pub trait BuiltInAppModule {\n  // 注入上下文\n  fn initialize(\n    &mut self,\n    context: &ModuleContext,\n  ) -> Result<(), String>;\n  fn start(&mut self) -> Result<std::sync::Arc<dyn ModuleService>, String>;\n}';
  problems = compareTraits(extractTraitMethods(prod), extractTraitMethods(reformatted));
  if (problems.length !== 0) throw new Error(`C1 自检失败：等价格式重排被误报：${problems.join('; ')}`);
  // C3 自检：字段类型漂移必须失败，注释/空白变化不误报。
  const ctxProd = 'pub struct ModuleContext { pub app_id: String, /* 目录 */ pub module_data_root: PathBuf }';
  const fields = extractStructFields(ctxProd, 'ModuleContext');
  if (fields.app_id !== 'String' || fields.module_data_root !== 'PathBuf') throw new Error('C3 自检失败：字段类型提取漂移');
  console.log('契约门禁自检通过（C1 参数/返回/receiver 漂移正反例、等价格式反例、C3 字段类型）');
}

// ModuleContext 字段 pinned 契约表（T01 二次实施）：类型变化必须显式更新
// 本表并同步技术 07——这是有意的漂移检测点，不是可以静默放宽的白名单。
const CONTEXT_FIELD_TYPES = {
  app_id: 'String',
  product_version: 'String',
  module_data_root: 'PathBuf',
  imports_root: 'PathBuf',
  cache_root: 'PathBuf',
  logs_root: 'PathBuf',
  keychain_namespace: 'String',
  secrets: 'std::sync::Arc<crate::secrets::SecretStore>',
  activation_generation: 'u64',
  cancellation: 'std::sync::Arc<crate::cancellation::CancellationToken>',
  limits: 'crate::limits::RuntimeLimits',
};

assertSelfTests();

// ---------- 真实文件检查 ----------

const moduleRs = read('crates/app-runtime-core/src/module.rs');
const tech07 = read('docs/standards/technical/07-built-in-app-development.md');
const contract = read('docs/contracts/managed-app-contract.md');
let registry = null;
try {
  registry = JSON.parse(read('modules/registry.json'));
} catch {
  fail('modules/registry.json 解析失败');
}

// C1/C2：生产 ⇄ 技术 07 §3 代码块。
const docBlockRaw = (tech07.split('```rust')[1] || '').split('```')[0] || '';
if (!docBlockRaw) fail('技术 07 缺少 rust 契约代码块（§3 模块 Rust 契约接口）');
const productionTraits = extractTraitMethods(moduleRs);
const documentedTraits = extractTraitMethods(docBlockRaw);
for (const problem of compareTraits(productionTraits, documentedTraits)) fail(problem);

// 生命周期 trait 方法集合与 501 默认语义。
const service = productionTraits.ModuleService || {};
const lifecycle = productionTraits.BuiltInAppModule || {};
for (const m of ['handle_ui', 'handle_api', 'card_snapshot']) if (!service[m]) fail(`module.rs ModuleService 缺少方法 ${m}`);
for (const m of ['descriptor', 'initialize', 'start', 'data_status', 'health', 'shutdown']) if (!lifecycle[m]) fail(`module.rs BuiltInAppModule 缺少方法 ${m}`);
if (!/start\(&mutself\)->Result<Arc<dynModuleService>,String>/.test((lifecycle.start || {}).sig || '')) {
  fail(`start() 签名必须为 start(&mut self) -> Result<Arc<dyn ModuleService>, String>（ADR-0039 决策 5；实际 ${(lifecycle.start || {}).sig || '缺失'}）`);
}
for (const [label, source] of [['module.rs', moduleRs], ['技术 07 §3', docBlockRaw]]) {
  if (!source.includes('APP_CARD_SNAPSHOT_UNSUPPORTED')) fail(`C4：${label} card_snapshot 默认实现必须保留 APP_CARD_SNAPSHOT_UNSUPPORTED`);
  if (!/card_snapshot[\s\S]{0,240}Err\(\(?501\s*,/.test(source.replace(/\s+/g, ' '))) fail(`C4：${label} card_snapshot 默认必须返回 (501, APP_CARD_SNAPSHOT_UNSUPPORTED)`);
}

// C3：ModuleContext 字段类型 ⇄ pinned 表。
const ctxFields = extractStructFields(moduleRs, 'ModuleContext');
if (!ctxFields) fail('module.rs 缺少 pub struct ModuleContext');
else {
  for (const [field, expected] of Object.entries(CONTEXT_FIELD_TYPES)) {
    if (!(field in ctxFields)) fail(`C3：ModuleContext 缺少字段 ${field}（技术 07 §3 注入要求）`);
    else if (ctxFields[field] !== normalize(expected)) {
      fail(`C3：ModuleContext.${field} 类型漂移：生产 ${ctxFields[field]} vs 契约表 ${normalize(expected)}（如需变更请先更新技术 07 与本表并记录原因）`);
    }
  }
  const ctxDocSection = tech07.split('### ModuleContext 注入要求')[1] || '';
  if (!ctxDocSection) fail('技术 07 缺少「ModuleContext 注入要求」章节');
  else for (const field of Object.keys(ctxFields)) {
    if (!ctxDocSection.includes(field)) fail(`技术 07 ModuleContext 注入要求缺少字段 ${field}`);
  }
}

// C6：托管契约 §14 语义声明。
if (!contract.includes('`start()` 返回 `Arc<dyn ModuleService>`')) fail('managed-app-contract §14 必须声明 start() 返回 Arc<dyn ModuleService>');
if (!contract.includes('不在生命周期锁中处理业务 HTTP')) fail('managed-app-contract §14 必须声明 HTTP 不在生命周期锁中处理业务');

// C5：registry 静态工厂 ⇄ 模块源码 BuiltInAppModule 实现。
if (registry && Array.isArray(registry.apps)) {
  for (const app of registry.apps) {
    const m = /^(\w+)::(\w+)::new$/.exec(app.factory || '');
    if (!m) {
      fail(`registry 工厂字段非法：${app.appId} -> ${app.factory}`);
      continue;
    }
    const srcDir = join(ROOT, 'modules', app.appId, 'src');
    if (!existsSync(srcDir)) {
      fail(`registry appId 无模块源码目录：modules/${app.appId}/src`);
      continue;
    }
    let hasStruct = false, hasImpl = false;
    const stack = [srcDir];
    while (stack.length) {
      const dir = stack.pop();
      for (const ent of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, ent.name);
        if (ent.isDirectory()) stack.push(full);
        else if (ent.name.endsWith('.rs')) {
          const src = readFileSync(full, 'utf8');
          if (src.includes(`struct ${m[2]}`)) hasStruct = true;
          if (src.match(new RegExp(`impl\\s+(?:\\w+::)*BuiltInAppModule\\s+for\\s+${m[2]}\\b`))) hasImpl = true;
        }
      }
    }
    if (!hasStruct) fail(`registry 工厂类型未声明：${app.factory}（无 struct ${m[2]}）`);
    if (!hasImpl) fail(`registry 工厂类型未实现 BuiltInAppModule：${app.factory}（C5）`);
  }
}

if (violations.length) {
  console.error('模块契约一致性检查失败：');
  for (const v of violations) console.error(`  - ${v}`);
  process.exit(1);
}
console.log(`模块契约一致性检查通过：C1 双侧签名逐字符一致（ModuleService[${Object.keys(service)}] BuiltInAppModule[${Object.keys(lifecycle)}]）、C2 归属一致、C3 ModuleContext ${Object.keys(CONTEXT_FIELD_TYPES).length} 字段类型一致、C4 501 默认语义、C5 双工厂 BuiltInAppModule 实现、C6 契约 §14 语义`);
