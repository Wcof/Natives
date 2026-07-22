import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import {
  CONFIGURABLE_API_PROTOCOLS,
  CONFIGURABLE_PROVIDER_PRESETS,
  DEFAULT_API_PROTOCOL,
  resolvePresetProtocol,
} from './provider-presets';

const dialog = readFileSync(new URL('../components/settings/AddProviderDialog.tsx', import.meta.url), 'utf8');
const zh = readFileSync(new URL('../i18n/zh.ts', import.meta.url), 'utf8');
const en = readFileSync(new URL('../i18n/en.ts', import.meta.url), 'utf8');

describe('add provider dialog contract', () => {
  it('uses accessible provider options', () => {
    assert.doesNotMatch(dialog, /<div key=\{preset\.name\} onClick=/);
    assert.match(dialog, /<button[\s\S]*className="add-provider-option/);
  });

  it('keeps protocol selection in advanced options and only exposes testable protocols', () => {
    assert.match(dialog, /CONFIGURABLE_PROVIDER_PRESETS/);
    assert.match(dialog, /CONFIGURABLE_API_PROTOCOLS/);
    assert.match(dialog, /settings\.advancedOptions/);
    assert.match(dialog, /settings\.apiProtocol/);
    assert.match(dialog, /<select[^>]*value=\{selectedProtocol\}/);
    assert.match(dialog, /value=\{protocol\}/);
    assert.equal(CONFIGURABLE_API_PROTOCOLS.join(','), 'anthropic_messages,openai_chat_completions,openai_responses');
    assert.doesNotMatch(dialog, /value="gemini_generate_content"/);
    assert.doesNotMatch(dialog, /value="ollama_chat"/);
    assert.doesNotMatch(dialog, /selected\.protocol \?\? 'openai_compatible'/);
  });

  it('labels the discover, test, and save sequence', () => {
    assert.match(dialog, /add-provider-steps/);
    assert.match(dialog, /settings\.fetchModels/);
    assert.match(dialog, /assistant\.testConnection/);
  });

  it('localizes protocol labels and setup intro for both Anthropic and OpenAI', () => {
    assert.match(zh, /apiProtocolAnthropic/);
    assert.match(en, /apiProtocolAnthropic/);
    assert.match(zh, /支持 Anthropic Messages 与 OpenAI 兼容协议/);
    assert.match(en, /Anthropic Messages and OpenAI-compatible protocols are supported/);
  });
});

describe('provider preset protocol defaults', () => {
  it('annotates every preset with a configurable protocol', () => {
    for (const preset of CONFIGURABLE_PROVIDER_PRESETS) {
      assert.ok(preset.protocol, `${preset.name} is missing protocol`);
      assert.ok(
        (CONFIGURABLE_API_PROTOCOLS as readonly string[]).includes(preset.protocol!),
        `${preset.name} uses unsupported protocol ${preset.protocol}`,
      );
    }
  });

  it('defaults custom providers to OpenAI and Claude-style presets to Anthropic', () => {
    const custom = CONFIGURABLE_PROVIDER_PRESETS.find((preset) => preset.name === 'Custom Provider');
    const claude = CONFIGURABLE_PROVIDER_PRESETS.find((preset) => preset.name === 'Claude Official');
    const deepseek = CONFIGURABLE_PROVIDER_PRESETS.find((preset) => preset.name === 'DeepSeek');
    const silicon = CONFIGURABLE_PROVIDER_PRESETS.find((preset) => preset.name === 'SiliconFlow');

    assert.equal(resolvePresetProtocol(custom), 'openai_chat_completions');
    assert.equal(resolvePresetProtocol(claude), 'anthropic_messages');
    assert.equal(resolvePresetProtocol(deepseek), 'anthropic_messages');
    assert.equal(resolvePresetProtocol(silicon), 'openai_chat_completions');
    assert.equal(DEFAULT_API_PROTOCOL, 'anthropic_messages');
  });
});

describe('provider test failure UX', () => {
  it('classifies the raw 429 diagnostic dump into a friendly Chinese rate-limit message', async () => {
    const { normalizeProviderTest } = await import('./tauri-adapter');
    const raw =
      'Provider test failed: protocol=openai_chat_completions, model=deepseek-v4-flash, http_status=429, retryable=true, request_id=2e75f801-ffc9-42d0-aad5-22b9179b355b, message=Rate limited — too many requests';

    const result = normalizeProviderTest({ success: false, error: raw });

    assert.equal(result.success, false);
    assert.equal(result.status, 'rate_limited');
    assert.equal(result.errorCode, 'RATE_LIMITED');
    assert.equal(result.userMessage, '请求过于频繁，已被限流');
    assert.doesNotMatch(result.userMessage ?? '', /protocol=/);
    assert.doesNotMatch(result.userMessage ?? '', /http_status=/);
  });

  it('keeps non-rate-limit failures classified without dumping protocol metadata', async () => {
    const { normalizeProviderTest } = await import('./tauri-adapter');
    const raw =
      'Provider test failed: protocol=openai_chat_completions, model=deepseek-v4-flash, http_status=401, retryable=false, request_id=none, message=Authentication failed — invalid API key';

    const result = normalizeProviderTest({ success: false, error: raw });

    assert.equal(result.success, false);
    assert.equal(result.status, 'invalid');
    assert.equal(result.userMessage, '身份验证失败');
    assert.doesNotMatch(result.userMessage ?? '', /protocol=/);
  });
});
