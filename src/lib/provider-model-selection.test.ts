import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  classifyProviderReadiness,
  connectionFingerprint,
  normalizeDiscoveredModels,
  selectDiscoveredModel,
  canTestDiscoveredModel,
  selectAssistantModel,
  resolveModelSelection,
  unwrapProviderListPayload,
  mapWireProviders,
  toProviderInfo,
} from './provider-model-selection';

describe('classifyProviderReadiness', () => {
  it('no_provider', () => assert.equal(classifyProviderReadiness([]), 'no_provider'));
  it('no_model', () =>
    assert.equal(
      classifyProviderReadiness([
        { id: 'p1', provider_type: 'o', display_name: 'O', has_active_key: false, models: [] },
      ]),
      'no_model',
    ));
  it('ready', () =>
    assert.equal(
      classifyProviderReadiness([
        {
          id: 'p1',
          provider_type: 'o',
          display_name: 'O',
          has_active_key: true,
          models: [{ id: 'gpt-4' }],
        },
      ]),
      'ready',
    ));
});

describe('selectAssistantModel', () => {
  it('null empty', () => assert.equal(selectAssistantModel([]), null));
  it('first ready', () => {
    const r = selectAssistantModel([
      {
        id: 'p1',
        provider_type: 'o',
        display_name: 'O',
        has_active_key: true,
        default_model: 'gpt-4o',
        models: [{ id: 'gpt-4o' }],
      },
    ]);
    assert.notEqual(r, null);
    assert.equal(r!.providerId, 'p1');
    assert.equal(r!.modelId, 'gpt-4o');
  });
});

describe('connectionFingerprint', () => {
  it('same', () =>
    assert.equal(
      connectionFingerprint('https://a.com', 'sk-abc'),
      connectionFingerprint('https://a.com', 'sk-abc'),
    ));
  it('diff', () =>
    assert.notEqual(
      connectionFingerprint('https://a.com', 'sk-abc'),
      connectionFingerprint('https://a.com', 'sk-xyz'),
    ));
});

describe('normalizeDiscoveredModels', () => {
  it('filters', () => {
    const r = normalizeDiscoveredModels([{ id: 'gpt-4' }, { id: '' }, { id: '  ' }]);
    assert.equal(r.length, 1);
    assert.equal(r[0]!.id, 'gpt-4');
  });
});

describe('selectDiscoveredModel', () => {
  it('first', () => {
    const r = selectDiscoveredModel([{ id: 'gpt-4' }, { id: 'claude' }]);
    assert.notEqual(r, null);
    assert.equal(r!.id, 'gpt-4');
  });
  it('null empty', () => assert.equal(selectDiscoveredModel([]), null));
});

describe('canTestDiscoveredModel', () => {
  it('true', () => assert.equal(canTestDiscoveredModel([{ id: 'gpt-4' }]), true));
});

describe('unwrapProviderListPayload', () => {
  it('accepts bare arrays from fixtures', () => {
    const rows = unwrapProviderListPayload([
      { id: 'openai', display_name: 'OpenAI', has_active_key: true, models: [{ id: 'gpt-4o' }] },
    ]);
    assert.equal(rows.length, 1);
    assert.equal(rows[0]!.id, 'openai');
  });

  it('accepts production { providers: [...] } envelopes', () => {
    const rows = unwrapProviderListPayload({
      providers: [
        {
          id: 'p1',
          display_name: 'SenseNova',
          has_active_key: true,
          models: [{ id: 'deepseek-v4-flash', display_name: 'DeepSeek V4 Flash' }],
        },
      ],
    });
    assert.equal(rows.length, 1);
    assert.equal(rows[0]!.id, 'p1');
  });

  it('returns empty for unexpected shapes', () => {
    assert.deepEqual(unwrapProviderListPayload(null), []);
    assert.deepEqual(unwrapProviderListPayload({}), []);
    assert.deepEqual(unwrapProviderListPayload('x'), []);
  });
});

describe('mapWireProviders', () => {
  it('maps envelope providers into picker options and readiness ready', () => {
    const mapped = mapWireProviders({
      providers: [
        {
          id: 'p1',
          provider_type: 'openai_compatible',
          display_name: 'SenseNova',
          api_base_url: 'https://api.sensenova.cn/v1',
          has_active_key: true,
          default_model: 'deepseek-v4-flash',
          models: [{ id: 'deepseek-v4-flash', display_name: 'DeepSeek V4 Flash' }],
        },
      ],
    });
    assert.equal(mapped.length, 1);
    assert.equal(mapped[0]!.id, 'p1');
    assert.equal(mapped[0]!.models[0]!.id, 'deepseek-v4-flash');
    assert.equal(mapped[0]!.keys.length, 1);
    assert.equal(classifyProviderReadiness(toProviderInfo(mapped)), 'ready');
  });

  it('treats missing active key as no_model readiness', () => {
    const mapped = mapWireProviders({
      providers: [
        {
          id: 'p1',
          display_name: 'Empty Key',
          has_active_key: false,
          models: [{ id: 'm1' }],
        },
      ],
    });
    assert.equal(mapped[0]!.keys.length, 0);
    assert.equal(classifyProviderReadiness(toProviderInfo(mapped)), 'no_model');
  });

  it('dedupes providers with same id and collapses same name+baseUrl ghosts', () => {
    const mapped = mapWireProviders({
      providers: [
        {
          id: 'p1',
          display_name: 'DeepSeek',
          api_base_url: 'https://api.deepseek.com',
          has_active_key: true,
          models: [
            { id: 'deepseek-chat', display_name: 'DeepSeek Chat' },
            { id: 'deepseek-chat', display_name: 'DeepSeek Chat' },
            { id: 'deepseek-reasoner' },
          ],
        },
        {
          id: 'p1',
          display_name: 'DeepSeek',
          api_base_url: 'https://api.deepseek.com',
          has_active_key: false,
          models: [{ id: 'deepseek-chat' }],
        },
        {
          id: 'p2',
          display_name: 'DeepSeek',
          api_base_url: 'https://api.deepseek.com/',
          has_active_key: true,
          models: [{ id: 'deepseek-chat' }, { id: 'deepseek-reasoner' }],
        },
      ],
    });
    assert.equal(mapped.length, 1);
    assert.equal(mapped[0]!.models.length, 2);
    assert.ok(mapped[0]!.keys.length > 0);
  });
});

describe('resolveModelSelection', () => {
  const providers = mapWireProviders({
    providers: [
      {
        id: 'p1',
        display_name: 'SenseNova',
        has_active_key: true,
        default_model: 'deepseek-v4-flash',
        models: [
          { id: 'deepseek-v4-flash', display_name: 'Flash' },
          { id: 'deepseek-v4-pro', display_name: 'Pro' },
        ],
      },
      {
        id: 'p2',
        display_name: 'OpenAI',
        has_active_key: true,
        models: [{ id: 'gpt-4o' }],
      },
    ],
  });

  it('keeps an explicit provider+model pair', () => {
    const r = resolveModelSelection(providers, {
      providerId: 'p2',
      modelId: 'gpt-4o',
    });
    assert.deepEqual(r, { providerId: 'p2', modelId: 'gpt-4o' });
  });

  it('falls back to provider default when model missing', () => {
    const r = resolveModelSelection(providers, {
      providerId: 'p1',
      modelId: '',
    });
    assert.deepEqual(r, { providerId: 'p1', modelId: 'deepseek-v4-flash' });
  });

  it('blocks stale provider ids instead of remapping to another host', () => {
    const r = resolveModelSelection(providers, {
      providerId: 'ghost-id',
      modelId: 'gpt-4o',
    });
    assert.equal(r, null);
  });

  it('defaults to first ready provider when preference empty', () => {
    const r = resolveModelSelection(providers, { providerId: '', modelId: '' });
    assert.deepEqual(r, { providerId: 'p1', modelId: 'deepseek-v4-flash' });
  });
});
