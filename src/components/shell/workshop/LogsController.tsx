'use client';

import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react';
import AppLogsPanel from '@/components/creative/AppLogsPanel';
import { classifyError } from '@/lib/error-classifier';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface LogsControllerHandle {
  open: (app: CreativeAppSummary) => void;
}

export interface LogsControllerProps {
  onToast: (message: string) => void;
}

/**
 * Runtime-scoped log viewer (CR-301). Owns the logs buffer (bounded), filter,
 * auto-scroll, and the live `onLog` subscription so a late event from an old
 * run can never appear in the current run's viewer. Exposes `open(app)` to the
 * catalog so one controller drives one log surface.
 */
const LogsController = forwardRef<LogsControllerHandle, LogsControllerProps>(
  function LogsController({ onToast }, ref) {
    const [logsFor, setLogsFor] = useState<CreativeAppSummary | null>(null);
    const [logsText, setLogsText] = useState('');
    const [logAutoScroll, setLogAutoScroll] = useState(true);
    const [logFilter, setLogFilter] = useState('');
    const logPreRef = useRef<HTMLPreElement | null>(null);

    const openLogs = useCallback(async (app: CreativeAppSummary) => {
      setLogsFor(app);
      setLogsText('…');
      setLogAutoScroll(true);
      // Runtime-scoped logs: prefer the active runtime instance id so a late
      // event from an old run can never appear in the current run's viewer.
      const logKey = app.runtimeInstanceId ?? app.id;
      try {
        if (app.source === 'local_project' && window.nativesAPI?.creativeApp?.getLocalLogs) {
          const lines = await window.nativesAPI.creativeApp.getLocalLogs(logKey, 400);
          if (Array.isArray(lines) && lines.length > 0) {
            setLogsText(lines.map((l) => `[${l.stream}] ${l.text}`).join('\n'));
            return;
          }
        }
        const text = await window.nativesAPI?.creativeApp?.logs?.(logKey, 200);
        setLogsText(text || '');
      } catch (err) {
        setLogsText(classifyError(err).userMessage);
      }
    }, []);

    useImperativeHandle(ref, () => ({ open: openLogs }), [openLogs]);

    useEffect(() => {
      if (!logsFor) return;
      const api = window.nativesAPI?.creativeApp;
      if (!api?.onLog) return;
      return api.onLog((ev) => {
        const matches = logsFor.runtimeInstanceId
          ? ev.runtimeId != null && ev.runtimeId === logsFor.runtimeInstanceId
          : ev.appId === logsFor.id;
        if (!matches) return;
        setLogsText((prev) => {
          const line = `[${ev.stream}] ${ev.text}`;
          if (!prev || prev === '…') return line;
          const next = `${prev}\n${line}`;
          // Bound the Renderer log buffer so a long-lived server cannot grow
          // it without limit. Drop the oldest once past the cap.
          const CAP = 200_000;
          return next.length > CAP ? next.slice(next.length - CAP) : next;
        });
      });
    }, [logsFor]);

    useEffect(() => {
      if (!logsFor || !logAutoScroll) return;
      const el = logPreRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    }, [logsText, logsFor, logAutoScroll]);

    return (
      <AppLogsPanel
        app={logsFor}
        logsText={logsText}
        logFilter={logFilter}
        logAutoScroll={logAutoScroll}
        preRef={logPreRef}
        onClose={() => setLogsFor(null)}
        onSetFilter={setLogFilter}
        onSetAutoScroll={setLogAutoScroll}
        onClear={() => setLogsText('')}
        onRefresh={() => {
          if (logsFor) void openLogs(logsFor);
        }}
        onToast={onToast}
      />
    );
  },
);

export default LogsController;
