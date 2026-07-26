'use client';

/**
 * JobFormModal — 任务创建/编辑表单
 *
 * 契约：job_create / job_update（第 5 节）。必填 name / schedule_type /
 * schedule_value / project_path / prompt；后端错误码
 * JOB_INVALID_SCHEDULE / JOB_INVALID_PROJECT_PATH 映射为对应字段错误。
 */

import { useState } from 'react';
import { t, useLocale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { fmtDateTime } from '@/lib/format';
import {
  jobCreate,
  jobUpdate,
  extractJobErrorCode,
  type JobDetail,
  type JobPayload,
  type JobPermissionProfile,
  type JobScheduleType,
} from '@/lib/jobs-api';
import {
  validateJobForm,
  validateCronExpression,
  nextCronRuns,
  localInputToIso,
  isoToLocalInput,
  gmtOffsetLabel,
  type JobFormErrors,
  type JobFormField,
} from '@/lib/jobs-view';

/** FormState 字段 → 校验错误字段的映射（用于输入即清除对应错误） */
const ERROR_FIELD_OF: Partial<Record<keyof FormState, JobFormField>> = {
  name: 'name',
  prompt: 'prompt',
  project_path: 'project_path',
  once_value: 'schedule_value',
  interval_value: 'schedule_value',
  cron_value: 'schedule_value',
  schedule_type: 'schedule_value',
};

const SCHEDULE_TYPES: JobScheduleType[] = ['once', 'interval', 'cron'];
const PERMISSION_PROFILES: JobPermissionProfile[] = ['readonly', 'ask', 'full_access'];

interface JobFormModalProps {
  open: boolean;
  /** 编辑时传入详情；新建为 null */
  initial: JobDetail | null;
  onClose: () => void;
  onSaved: (job: JobDetail, created: boolean) => void;
}

interface FormState {
  name: string;
  description: string;
  project_path: string;
  prompt: string;
  schedule_type: JobScheduleType;
  /** once 型存 datetime-local 本地值，提交时转 ISO8601 */
  once_value: string;
  interval_value: string;
  cron_value: string;
  permission_profile: JobPermissionProfile;
}

function initialFormState(initial: JobDetail | null): FormState {
  const type = (initial?.schedule_type ?? 'interval') as JobScheduleType;
  return {
    name: initial?.name ?? '',
    description: initial?.description ?? '',
    project_path: initial?.project_path ?? '',
    prompt: initial?.prompt ?? '',
    schedule_type: SCHEDULE_TYPES.includes(type) ? type : 'interval',
    once_value: type === 'once' && initial ? isoToLocalInput(initial.schedule_value) : '',
    interval_value: type === 'interval' && initial ? initial.schedule_value : '',
    cron_value: type === 'cron' && initial ? initial.schedule_value : '',
    permission_profile: PERMISSION_PROFILES.includes(
      initial?.permission_profile as JobPermissionProfile,
    )
      ? (initial?.permission_profile as JobPermissionProfile)
      : 'readonly',
  };
}

function rawScheduleValue(form: FormState): string {
  switch (form.schedule_type) {
    case 'once':
      // 校验层面用本地值即可（Date.parse 同样接受），提交时再转 UTC
      return form.once_value;
    case 'interval':
      return form.interval_value;
    case 'cron':
      return form.cron_value;
  }
}

const FIELD_LABEL_STYLE: React.CSSProperties = {
  display: 'block',
  fontSize: '0.75rem',
  fontWeight: 600,
  color: 'var(--text-secondary)',
  marginBottom: 4,
};

function FieldError({ msg }: { msg?: string }) {
  if (!msg) return null;
  return (
    <div role="alert" style={{ color: 'var(--danger)', fontSize: '0.75rem', marginTop: 4 }}>
      {msg}
    </div>
  );
}

/** 编辑中切走视图会整体卸载 JobsPage；草稿落 sessionStorage 以便返回后恢复。 */
function draftStorageKey(initial: JobDetail | null): string {
  return `natives:jobform-draft:${initial?.id ?? '__new__'}`;
}

function readDraft(initial: JobDetail | null): FormState | null {
  try {
    const raw = window.sessionStorage.getItem(draftStorageKey(initial));
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<FormState>;
    // 以初始态为底合并，防旧草稿缺字段
    return { ...initialFormState(initial), ...parsed };
  } catch {
    return null;
  }
}

export default function JobFormModal({ open, initial, onClose, onSaved }: JobFormModalProps) {
  const locale = useLocale();
  // 表单重置依赖父级 key 重挂载（open/编辑对象变化时换 key），不在 effect 里同步 setState。
  // 挂载时优先恢复视图切换前遗留的草稿（显式取消/保存成功才清除）。
  const [form, setForm] = useState<FormState>(() => (open ? readDraft(initial) : null) ?? initialFormState(initial));
  const [errors, setErrors] = useState<JobFormErrors>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const clearDraft = () => {
    try { window.sessionStorage.removeItem(draftStorageKey(initial)); } catch { /* ignore */ }
  };

  const set = <K extends keyof FormState>(key: K, value: FormState[K]) => {
    setForm((prev) => {
      const next = { ...prev, [key]: value };
      try { window.sessionStorage.setItem(draftStorageKey(initial), JSON.stringify(next)); } catch { /* ignore */ }
      return next;
    });
    // 输入即清除该字段的校验错误，避免改正后错误滞留到下次提交
    const errorField = ERROR_FIELD_OF[key];
    if (errorField) {
      setErrors((prev) => {
        if (!prev[errorField]) return prev;
        const next = { ...prev };
        delete next[errorField];
        return next;
      });
    }
  };

  // 渲染期禁止取当前时间（react-hooks 纯函数约束），预览与过期提示均在输入事件里计算
  const [cronPreview, setCronPreview] = useState<Date[] | null>(null);
  const [oncePast, setOncePast] = useState(false);

  const recomputeCronPreview = (expr: string) => {
    const v = expr.trim();
    if (!v || !validateCronExpression(v)) {
      setCronPreview(null);
      return;
    }
    const runs = nextCronRuns(v, new Date(), 3);
    setCronPreview(runs.length > 0 ? runs : null);
  };

  const recomputeOncePast = (local: string) => {
    const ms = local ? Date.parse(local) : NaN;
    setOncePast(Number.isFinite(ms) && ms < Date.now());
  };

  const errText = (subKey?: string) =>
    subKey ? t(locale, `jobs.form.${subKey}`) : undefined;

  const handleSubmit = async () => {
    const scheduleValue = rawScheduleValue(form);
    const nextErrors = validateJobForm({
      name: form.name,
      prompt: form.prompt,
      project_path: form.project_path,
      schedule_type: form.schedule_type,
      schedule_value: scheduleValue,
    });
    setErrors(nextErrors);
    setFormError(null);
    if (Object.keys(nextErrors).length > 0) return;

    const submittedValue =
      form.schedule_type === 'once'
        ? localInputToIso(form.once_value) ?? form.once_value
        : scheduleValue.trim();

    const payload: JobPayload = {
      name: form.name.trim(),
      schedule_type: form.schedule_type,
      schedule_value: submittedValue,
      project_path: form.project_path.trim(),
      prompt: form.prompt,
      permission_profile: form.permission_profile,
    };
    const description = form.description.trim();
    if (description) payload.description = description;

    setSubmitting(true);
    try {
      const saved = initial
        ? await jobUpdate(initial.id, payload)
        : await jobCreate(payload);
      clearDraft();
      onSaved(saved, !initial);
      onClose();
    } catch (err) {
      const code = extractJobErrorCode(err);
      if (code === 'JOB_INVALID_SCHEDULE') {
        setErrors({ schedule_value: 'errInvalidBackendSchedule' });
      } else if (code === 'JOB_INVALID_PROJECT_PATH') {
        setErrors({ project_path: 'errInvalidBackendProjectPath' });
      } else if (code) {
        setFormError(t(locale, `jobs.errors.${code}`));
      } else {
        setFormError(
          t(locale, 'jobs.errors.unknown', {
            message: err instanceof Error ? err.message : String(err),
          }),
        );
      }
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      isOpen={open}
      onClose={() => { clearDraft(); onClose(); }}
      title={t(locale, initial ? 'jobs.form.editTitle' : 'jobs.form.createTitle')}
      width={560}
      closeOnEscape={!submitting}
      closeOnBackdropClick={!submitting}
      showCloseButton={!submitting}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        {/* 名称 */}
        <div>
          <label style={FIELD_LABEL_STYLE} htmlFor="job-form-name">
            {t(locale, 'jobs.form.name')}
          </label>
          <input
            id="job-form-name"
            className="input"
            value={form.name}
            onChange={(e) => set('name', e.target.value)}
            placeholder={t(locale, 'jobs.form.namePlaceholder')}
          />
          <FieldError msg={errText(errors.name)} />
        </div>

        {/* 描述（可选） */}
        <div>
          <label style={FIELD_LABEL_STYLE} htmlFor="job-form-description">
            {t(locale, 'jobs.form.description')}
          </label>
          <input
            id="job-form-description"
            className="input"
            value={form.description}
            onChange={(e) => set('description', e.target.value)}
          />
        </div>

        {/* 项目路径 */}
        <div>
          <label style={FIELD_LABEL_STYLE} htmlFor="job-form-project-path">
            {t(locale, 'jobs.form.projectPath')}
          </label>
          <input
            id="job-form-project-path"
            className="input"
            value={form.project_path}
            onChange={(e) => set('project_path', e.target.value)}
            placeholder={t(locale, 'jobs.form.projectPathPlaceholder')}
            spellCheck={false}
          />
          <div style={{ fontSize: '0.6875rem', color: 'var(--text-disabled)', marginTop: 4 }}>
            {t(locale, 'jobs.form.projectPathHint')}
          </div>
          <FieldError msg={errText(errors.project_path)} />
        </div>

        {/* 提示词 */}
        <div>
          <label style={FIELD_LABEL_STYLE} htmlFor="job-form-prompt">
            {t(locale, 'jobs.form.prompt')}
          </label>
          <textarea
            id="job-form-prompt"
            className="input"
            style={{ minHeight: 88, resize: 'vertical' }}
            value={form.prompt}
            onChange={(e) => set('prompt', e.target.value)}
            placeholder={t(locale, 'jobs.form.promptPlaceholder')}
          />
          <FieldError msg={errText(errors.prompt)} />
        </div>

        {/* 调度类型（三态切换） */}
        <div>
          <span style={FIELD_LABEL_STYLE}>{t(locale, 'jobs.form.scheduleType')}</span>
          <div role="radiogroup" aria-label={t(locale, 'jobs.form.scheduleType')} style={{ display: 'flex', gap: 6 }}>
            {SCHEDULE_TYPES.map((type) => {
              const active = form.schedule_type === type;
              return (
                <button
                  key={type}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() => set('schedule_type', type)}
                  className="btn"
                  style={{
                    padding: '4px 12px',
                    fontSize: '0.8125rem',
                    background: active ? 'var(--accent)' : 'transparent',
                    color: active ? 'var(--accent-ink)' : 'var(--text-secondary)',
                    border: '1px solid var(--border)',
                    borderRadius: 8,
                    cursor: 'pointer',
                  }}
                >
                  {t(locale, `jobs.scheduleType.${type}`)}
                </button>
              );
            })}
          </div>
        </div>

        {/* 调度值 — 按类型切换控件 */}
        {form.schedule_type === 'once' && (
          <div>
            <label style={FIELD_LABEL_STYLE} htmlFor="job-form-once">
              {t(locale, 'jobs.form.onceValue')}
            </label>
            <input
              id="job-form-once"
              className="input"
              type="datetime-local"
              value={form.once_value}
              onChange={(e) => {
                set('once_value', e.target.value);
                recomputeOncePast(e.target.value);
              }}
            />
            <div style={{ fontSize: '0.6875rem', color: 'var(--text-disabled)', marginTop: 4 }}>
              {t(locale, 'jobs.form.timezoneHint', { offset: gmtOffsetLabel() })}
            </div>
            {oncePast && (
              <div style={{ fontSize: '0.6875rem', color: 'var(--warning)', marginTop: 4 }}>
                {t(locale, 'jobs.form.oncePastHint')}
              </div>
            )}
            <FieldError msg={errText(errors.schedule_value)} />
          </div>
        )}
        {form.schedule_type === 'interval' && (
          <div>
            <label style={FIELD_LABEL_STYLE} htmlFor="job-form-interval">
              {t(locale, 'jobs.form.intervalValue')}
            </label>
            <input
              id="job-form-interval"
              className="input"
              type="number"
              min={60}
              step={1}
              value={form.interval_value}
              onChange={(e) => set('interval_value', e.target.value)}
            />
            <div style={{ fontSize: '0.6875rem', color: 'var(--text-disabled)', marginTop: 4 }}>
              {t(locale, 'jobs.form.intervalHint')}
            </div>
            <FieldError msg={errText(errors.schedule_value)} />
          </div>
        )}
        {form.schedule_type === 'cron' && (
          <div>
            <label style={FIELD_LABEL_STYLE} htmlFor="job-form-cron">
              {t(locale, 'jobs.form.cronValue')}
            </label>
            <input
              id="job-form-cron"
              className="input"
              value={form.cron_value}
              onChange={(e) => {
                set('cron_value', e.target.value);
                recomputeCronPreview(e.target.value);
              }}
              placeholder="*/30 * * * *"
              spellCheck={false}
            />
            <div style={{ fontSize: '0.6875rem', color: 'var(--text-disabled)', marginTop: 4 }}>
              {t(locale, 'jobs.form.cronHint')}
            </div>
            {cronPreview && (
              <div style={{ fontSize: '0.6875rem', color: 'var(--text-secondary)', marginTop: 4 }}>
                {t(locale, 'jobs.form.cronNextRuns', { offset: gmtOffsetLabel() })}
                {cronPreview.map((d) => fmtDateTime(d.getTime())).join(' · ')}
              </div>
            )}
            <FieldError msg={errText(errors.schedule_value)} />
          </div>
        )}

        {/* 权限档位 */}
        <div>
          <label style={FIELD_LABEL_STYLE} htmlFor="job-form-permission">
            {t(locale, 'jobs.form.permissionProfile')}
          </label>
          <select
            id="job-form-permission"
            className="input"
            value={form.permission_profile}
            onChange={(e) => set('permission_profile', e.target.value as JobPermissionProfile)}
          >
            {PERMISSION_PROFILES.map((profile) => (
              <option key={profile} value={profile}>
                {t(locale, `jobs.form.permission.${profile}`)}
              </option>
            ))}
          </select>
        </div>

        {formError && (
          <div role="alert" style={{ color: 'var(--danger)', fontSize: '0.8125rem' }}>
            {formError}
          </div>
        )}

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 4 }}>
          <button type="button" className="btn-ghost" onClick={() => { clearDraft(); onClose(); }} disabled={submitting}>
            {t(locale, 'common.cancel')}
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void handleSubmit()}
            disabled={submitting}
          >
            {submitting
              ? t(locale, 'common.saving')
              : t(locale, initial ? 'jobs.form.submitSave' : 'jobs.form.submitCreate')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
