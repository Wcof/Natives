'use client';

import { useMemo, useState } from 'react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import { importCapabilityMcpJson } from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import Modal from '@/components/ui/Modal';
import { useToast } from '@/components/ui/Toast';
import type { McpJsonImportResult } from '../shared/capability-types';

interface ConnectorJsonImportDialogProps {
  locale: Locale;
  gateway: AssistantGateway;
  open: boolean;
  onClose: () => void;
  onImported: () => void;
}

/** Client-side preview: count server entries in a pasted mcpServers config. */
function previewServerNames(json: string): string[] | null {
  try {
    const parsed = JSON.parse(json) as unknown;
    if (!parsed || typeof parsed !== 'object') return null;
    const obj = parsed as Record<string, unknown>;
    const servers = obj.mcpServers && typeof obj.mcpServers === 'object' ? obj.mcpServers : obj;
    return Object.keys(servers as Record<string, unknown>);
  } catch {
    return null;
  }
}

/** Paste-and-import MCP servers from JSON (offline path; also the hub fallback). */
export default function ConnectorJsonImportDialog({
  locale,
  gateway,
  open,
  onClose,
  onImported,
}: ConnectorJsonImportDialogProps) {
  const { toast } = useToast();
  const [json, setJson] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<McpJsonImportResult | null>(null);
  const [formError, setFormError] = useState<string | null>(null);

  const previewNames = useMemo(() => (json.trim() ? previewServerNames(json) : null), [json]);

  const handleImport = async () => {
    if (!json.trim() || previewNames === null) {
      setFormError(t(locale, 'capabilities.connectors.jsonInvalid'));
      return;
    }
    setFormError(null);
    setSubmitting(true);
    try {
      const res = await importCapabilityMcpJson(gateway, json);
      setResult(res);
      if (res.imported.length > 0) {
        toast(t(locale, 'capabilities.connectors.jsonImported', { count: res.imported.length }), 'success');
        onImported();
      }
    } catch (e) {
      setFormError(classifyError(e).userMessage);
    } finally {
      setSubmitting(false);
    }
  };

  const handleClose = () => {
    setJson('');
    setResult(null);
    setFormError(null);
    onClose();
  };

  return (
    <Modal isOpen={open} onClose={handleClose} title={t(locale, 'capabilities.connectors.jsonImportTitle')} width={560}>
      <div className="space-y-3">
        <textarea
          value={json}
          onChange={(e) => {
            setJson(e.target.value);
            setResult(null);
          }}
          placeholder={t(locale, 'capabilities.connectors.jsonPlaceholder')}
          aria-label={t(locale, 'capabilities.connectors.jsonImportTitle')}
          className="h-48 w-full resize-none rounded border p-3 font-mono text-xs"
          style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
          autoFocus
        />

        {/* Parse preview */}
        {json.trim() ? (
          <div className="rounded-lg border p-2.5 text-xs" style={{ borderColor: 'var(--border-subtle)' }}>
            <span className="font-semibold" style={{ color: 'var(--text-secondary)' }}>
              {t(locale, 'capabilities.connectors.jsonPreview')}
            </span>
            {previewNames === null ? (
              <p className="mt-1" style={{ color: 'var(--danger)' }}>
                {t(locale, 'capabilities.connectors.jsonInvalid')}
              </p>
            ) : (
              <p className="mt-1" style={{ color: 'var(--text-secondary)' }}>
                {t(locale, 'capabilities.connectors.jsonServers', { count: previewNames.length })}
                {previewNames.length > 0 ? `：${previewNames.join(', ')}` : ''}
              </p>
            )}
          </div>
        ) : null}

        {/* Import result */}
        {result ? (
          <div className="rounded-lg border p-2.5 text-xs" style={{ borderColor: 'var(--border-subtle)' }}>
            <p style={{ color: 'var(--success, #10b981)' }}>
              {t(locale, 'capabilities.connectors.jsonImported', { count: result.imported.length })}
              {result.imported.length > 0 ? `：${result.imported.join(', ')}` : ''}
            </p>
            {result.skipped > 0 ? (
              <p style={{ color: 'var(--text-secondary)' }}>
                {t(locale, 'capabilities.connectors.jsonSkipped', { count: result.skipped })}
              </p>
            ) : null}
            {result.errors.length > 0 ? (
              <div style={{ color: 'var(--danger)' }}>
                <span>{t(locale, 'capabilities.connectors.jsonErrors')}:</span>
                <ul className="ml-4 list-disc">
                  {result.errors.map((err, i) => (
                    <li key={i}>{err}</li>
                  ))}
                </ul>
              </div>
            ) : null}
          </div>
        ) : null}

        {formError ? <p className="text-sm" style={{ color: 'var(--danger)' }}>{formError}</p> : null}

        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={handleClose}
            className="px-4 py-2 text-sm"
            style={{ color: 'var(--text-secondary)' }}
          >
            {t(locale, 'capabilities.common.close')}
          </button>
          <button
            type="button"
            onClick={() => void handleImport()}
            disabled={submitting || !json.trim()}
            className="rounded px-4 py-2 text-sm disabled:opacity-50"
            style={{ background: 'var(--primary)', color: '#fff' }}
          >
            {t(locale, 'capabilities.common.import')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
