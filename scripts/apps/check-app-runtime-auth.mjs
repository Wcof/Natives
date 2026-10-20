#!/usr/bin/env node
// App Runtime 鉴权与动态端口门禁（ADR-0035 / 架构 Wave 9 / 审查 A-03）：
// 验证 Release App Runtime 默认绑定动态端口，所有 /api/* 均严格强制要求 Bearer Token，无路由豁免。
import { readFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

const HTTP_CLIENT_RS = join(ROOT, 'crates', 'app-runtime', 'src', 'http', 'client.rs');
const HTTP_MOD_RS = join(ROOT, 'crates', 'app-runtime', 'src', 'http', 'mod.rs');

const violations = [];

function authenticatedContextIsRequired(text) {
  return text.includes('.authorized_session(request.header("authorization").unwrap_or(""))')
    && text.includes('app_runtime_core::module::RequestContext {')
    && /if\s+!request\.sandbox_origin\(\)\s*\|\|\s*context\.is_none\(\)\s*\{[\s\S]*?401,[\s\S]*?return;\s*\}/.test(text)
    && text.includes('service.handle_authenticated_api(&context.unwrap(), &request, route)')
    && !/service\.handle_api\s*\(/.test(text);
}

if (existsSync(HTTP_CLIENT_RS)) {
  const text = readFileSync(HTTP_CLIENT_RS, 'utf8');

  // 1. 验证不存在免 Bearer 路由例外（如 tray_readonly）
  if (text.includes('tray_readonly') || text.includes('/api/tray/state') && text.includes('!tray_readonly')) {
    violations.push('http/client.rs: 发现 /api/ 路由免鉴权特权逻辑，违反 ADR-0035');
  }

  // 2. ADR-0042: authorization must reach the module as trusted request context.
  if (!authenticatedContextIsRequired(text)) {
    violations.push('http/client.rs: 缺少 sandbox Origin、Bearer 与受信 RequestContext 的强制校验／传递');
  }
  assert.equal(authenticatedContextIsRequired(text.replace('context.is_none()', 'context.is_some()')), false, 'inverted authentication must fail');
  assert.equal(authenticatedContextIsRequired(text.replace('.authorized_session(', '.untrusted_session(')), false, 'unverified context must fail');
  assert.equal(authenticatedContextIsRequired(text.replace('service.handle_authenticated_api(&context.unwrap(), &request, route)', 'service.handle_api(&request, route)')), false, 'dropping Surface identity must fail');
}

if (existsSync(HTTP_MOD_RS)) {
  const text = readFileSync(HTTP_MOD_RS, 'utf8');

  // 3. 验证默认端口为 0（动态端口），无固定 8765 抢占
  if (/const\s+DEFAULT_PORT\s*:\s*u16\s*=\s*8765/.test(text) || /unwrap_or\(\s*8765\s*\)/.test(text)) {
    violations.push('http/mod.rs: 默认端口仍硬编码为 8765，违反 ADR-0035 纯动态端口规范');
  }
  if (!text.includes('unwrap_or(0)')) {
    violations.push('http/mod.rs: bind_loopback_listener 必须默认 fallback 到 0（操作系统动态分配）');
  }
}

if (violations.length > 0) {
  console.error('App Runtime 鉴权与端口门禁未通过：');
  for (const v of violations) console.error('  ' + v);
  process.exit(1);
}

console.log('✓ App Runtime auth & dynamic port gate passed (strict Bearer & ephemeral loopback enforced)');
