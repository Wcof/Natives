import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  isAbsolutePath,
  parseJobLastStatus,
  runStatusTone,
  scheduleSummarySpec,
  validateCronExpression,
  validateJobForm,
  validateScheduleValue,
  isoToLocalInput,
  localInputToIso,
} from './jobs-view';
import { JOB_RUN_STATUSES, extractJobErrorCode } from './jobs-api';

describe('validateScheduleValue（契约第 1 节 schedule 语义）', () => {
  it('once: ISO8601 有效，坏值报 errInvalidOnce', () => {
    assert.equal(validateScheduleValue('once', '2026-08-01T10:00:00Z'), null);
    assert.equal(validateScheduleValue('once', 'not-a-date'), 'errInvalidOnce');
    assert.equal(validateScheduleValue('once', ''), 'errRequired');
  });

  it('interval: 整数秒且最小 60', () => {
    assert.equal(validateScheduleValue('interval', '60'), null);
    assert.equal(validateScheduleValue('interval', '3600'), null);
    assert.equal(validateScheduleValue('interval', '59'), 'errIntervalMin');
    assert.equal(validateScheduleValue('interval', '1.5'), 'errInvalidInterval');
    assert.equal(validateScheduleValue('interval', 'abc'), 'errInvalidInterval');
  });

  it('cron: 5 字段，支持 * N */N A-B A,B,C', () => {
    assert.equal(validateCronExpression('* * * * *'), true);
    assert.equal(validateCronExpression('*/15 0 1-5 * 1,3,5'), true);
    assert.equal(validateCronExpression('0 12 * *'), false); // 4 字段
    assert.equal(validateCronExpression('a b c d e'), false);
    assert.equal(validateScheduleValue('cron', '0 3 * * *'), null);
  });
});

describe('validateJobForm（必填校验）', () => {
  it('缺失必填字段逐项标记', () => {
    const errors = validateJobForm({
      name: '',
      prompt: ' ',
      project_path: '',
      schedule_type: 'interval',
      schedule_value: '',
    });
    assert.deepEqual(errors, {
      name: 'errRequired',
      prompt: 'errRequired',
      project_path: 'errRequired',
      schedule_value: 'errRequired',
    });
  });

  it('project_path 必须为绝对路径', () => {
    assert.equal(isAbsolutePath('/Users/me/project'), true);
    assert.equal(isAbsolutePath('C:\\repo'), true);
    assert.equal(isAbsolutePath('relative/path'), false);
    const errors = validateJobForm({
      name: 'x',
      prompt: 'y',
      project_path: 'relative/path',
      schedule_type: 'interval',
      schedule_value: '60',
    });
    assert.equal(errors.project_path, 'errNotAbsolute');
  });
});

describe('parseJobLastStatus（scheduled_tasks.last_status）', () => {
  it('契约样例逐一映射', () => {
    assert.equal(parseJobLastStatus(null).key, 'never');
    assert.deepEqual(parseJobLastStatus('succeeded'), { key: 'succeeded', tone: 'success' });
    const failed = parseJobLastStatus('failed:ENGINE_TIMEOUT');
    assert.equal(failed.key, 'failed');
    assert.equal(failed.tone, 'danger');
    assert.equal(failed.detail, 'ENGINE_TIMEOUT');
    assert.equal(parseJobLastStatus('dispatch_error:not_wired').key, 'dispatchErrorNotWired');
    assert.equal(parseJobLastStatus('dispatch_error:other').key, 'dispatchError');
    assert.equal(parseJobLastStatus('expired').key, 'expired');
  });

  it('未知状态如实透传（raw）', () => {
    const parsed = parseJobLastStatus('weird_status');
    assert.equal(parsed.key, 'raw');
    assert.equal(parsed.detail, 'weird_status');
  });
});

describe('runStatusTone（状态机全集，契约第 2 节）', () => {
  it('每个冻结状态都有确定的色调', () => {
    const tones = ['success', 'danger', 'warning', 'neutral', 'info'];
    for (const status of JOB_RUN_STATUSES) {
      assert.ok(tones.includes(runStatusTone(status)), `tone for ${status}`);
    }
  });
});

describe('scheduleSummarySpec', () => {
  it('三种类型分别产出摘要参数', () => {
    assert.deepEqual(scheduleSummarySpec('interval', '3600'), {
      kind: 'interval',
      seconds: 3600,
      raw: '3600',
    });
    assert.deepEqual(scheduleSummarySpec('cron', '0 3 * * *'), {
      kind: 'cron',
      expr: '0 3 * * *',
    });
    const once = scheduleSummarySpec('once', '2026-08-01T10:00:00Z');
    assert.equal(once.kind, 'once');
    if (once.kind === 'once') {
      assert.equal(once.timeMs, Date.parse('2026-08-01T10:00:00Z'));
    }
  });
});

describe('once 值 datetime-local 互转', () => {
  it('ISO → local → ISO 保持同一时刻（分钟精度）', () => {
    const iso = '2026-08-01T10:30:00.000Z';
    const local = isoToLocalInput(iso);
    assert.match(local, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/);
    const roundTrip = localInputToIso(local);
    assert.notEqual(roundTrip, null);
    assert.equal(Date.parse(roundTrip!), Date.parse(iso));
  });

  it('无效输入返回空/null', () => {
    assert.equal(isoToLocalInput('bogus'), '');
    assert.equal(localInputToIso(''), null);
  });
});

describe('extractJobErrorCode（错误码子串提取，契约第 5 节）', () => {
  it('从 cmd() 包装后的消息中提取', () => {
    assert.equal(
      extractJobErrorCode(new Error('Tauri command failed: job_run_now — JOB_DISPATCHER_NOT_WIRED')),
      'JOB_DISPATCHER_NOT_WIRED',
    );
    assert.equal(
      extractJobErrorCode('Invalid input: JOB_INVALID_SCHEDULE: bad cron'),
      'JOB_INVALID_SCHEDULE',
    );
    assert.equal(extractJobErrorCode(new Error('something else')), null);
    assert.equal(extractJobErrorCode(undefined), null);
  });
});
