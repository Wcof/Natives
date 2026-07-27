'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, RefreshCw, Undo2, X } from 'lucide-react';
import { t } from '@/i18n';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { IFRAME_SANDBOX, assertSecureSandbox } from '@/lib/iframe-manager';
import { HttpPortNotAvailableError } from '@/lib/natives-http-port';
import { draftActions } from '@/lib/creative-draft';
import type { CreativeDraft } from '@/lib/tauri-adapter';

/** Why the L2 banner appeared. Fed back to the parent so the next turn can carry it. */
export type SoftFailureReason = 'load-error' | 'runtime-error' | 'load-timeout';

export interface DraftSoftFailure {
  reason: SoftFailureReason;
  /** Revision the signal belongs to — a later revision clears it. */
  revision: number;
  /** Only present for 'runtime-error': whatever the frame chose to report. */
  detail?: string;
}

interface DraftPreviewProps {
  draft: CreativeDraft | null;
  locale: string;
  /**
   * 会话侧的流式状态。draft.state 从未被引擎驱动（恒为 drafting），
   * 「生成中」以此实时信号为准。
   */
  generating?: boolean;
  /** Re-resolve the preview URL after the local HTTP service comes up. */
  onRetry?: () => void;
  /** Wired to the toolbar's undo so the L2 banner can act, not just warn. */
  onUndo?: () => void;
  /** Lets the session highlight its own undo entry and feed the error back to the model (section 10). */
  onSoftFailure?: (failure: DraftSoftFailure) => void;
}

/** URL resolution is its own axis: it fails independently of draft state. */
type UrlState = 'resolving' | 'ready' | 'port-unavailable';

export type PreviewPhase =
  | 'no-draft'
  | 'empty'
  | 'generating'
  | 'publishing'
  | 'port-unavailable'
  | 'loading'
  | 'ready';

/**
 * Overlay precedence, extracted so it is testable without a DOM.
 * In-flight draft states outrank transport problems: a draft that is still
 * generating has nothing to load yet, so "service not ready" would misdescribe it.
 */
export function resolvePreviewPhase(input: {
  draft: CreativeDraft | null;
  urlState: UrlState;
  frameLoaded: boolean;
  /** 会话流式中 = 生成中；draft.state 的 generating/publishing 无人驱动，仅作兜底。 */
  generating?: boolean;
}): PreviewPhase {
  const { draft, urlState, frameLoaded, generating } = input;
  if (!draft) return 'no-draft';
  // 首个版本尚不存在时优先显示「生成中」而非静态空状态
  if ((generating || draft.state === 'generating') && draft.currentRevision < 1) return 'generating';
  if (draft.state === 'publishing') return 'publishing';
  if (draft.currentRevision < 1) return 'empty';
  if (urlState === 'port-unavailable') return 'port-unavailable';
  if (urlState !== 'ready' || !frameLoaded) return 'loading';
  return 'ready';
}

/** Give a page this long to fire `load` before we call it suspicious. */
const LOAD_TIMEOUT_MS = 8000;

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="absolute inset-0 grid place-items-center bg-[var(--background)] px-6 text-center">
      <div className="flex flex-col items-center gap-3">{children}</div>
    </div>
  );
}

/**
 * Sandboxed preview of a draft's current revision.
 *
 * Security parity with published modules (design section 6): the frame runs on the
 * local HTTP origin under `IFRAME_SANDBOX`, which deliberately omits
 * `allow-same-origin`. That is also why this component can never read the
 * frame's `window` — see the L2 notes on `handleFrameMessage`.
 */
export default function DraftPreview({
  draft,
  locale,
  generating,
  onRetry,
  onUndo,
  onSoftFailure,
}: DraftPreviewProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [baseUrl, setBaseUrl] = useState<string | null>(null);
  const [urlState, setUrlState] = useState<UrlState>('resolving');
  const [frameLoaded, setFrameLoaded] = useState(false);
  const [softFailure, setSoftFailure] = useState<DraftSoftFailure | null>(null);

  const draftId = draft?.draftId ?? null;
  const revision = draft?.currentRevision ?? 0;
  const actions = draft ? draftActions(draft.state, draft.currentRevision) : null;

  // Keep the callback out of the signal effects' deps so a parent that passes an
  // inline closure cannot re-arm the load timeout on every render.
  const onSoftFailureRef = useRef(onSoftFailure);
  useEffect(() => {
    onSoftFailureRef.current = onSoftFailure;
  }, [onSoftFailure]);

  const raiseSoftFailure = useCallback(
    (reason: SoftFailureReason, detail?: string) => {
      const failure: DraftSoftFailure = { reason, revision, detail };
      setSoftFailure(failure);
      onSoftFailureRef.current?.(failure);
    },
    [revision],
  );

  const resolveUrl = useCallback(async () => {
    if (!draftId) return;
    setUrlState('resolving');
    try {
      const url = await window.nativesAPI?.creativeDraft?.previewUrl(draftId);
      if (!url) throw new HttpPortNotAvailableError();
      setBaseUrl(url);
      setUrlState('ready');
    } catch {
      // The port helper never falls back to a hardcoded port, so every failure
      // path here means the same thing to the user: nothing is serving yet.
      setBaseUrl(null);
      setUrlState('port-unavailable');
    }
  }, [draftId]);

  useEffect(() => {

    void resolveUrl();
  }, [resolveUrl]);

  // A new revision is a new verdict, so the previous one's warning must not
  // survive it. Adjusted during render rather than in an effect: an effect
  // would let one frame paint the stale banner over the new revision.
  const frameKey = `${draftId}:${revision}`;
  const [renderedFrameKey, setRenderedFrameKey] = useState(frameKey);
  if (renderedFrameKey !== frameKey) {
    setRenderedFrameKey(frameKey);
    setFrameLoaded(false);
    setSoftFailure(null);
  }

  // Absence of a `load` event is the only "nothing rendered" proxy available to
  // us across an opaque origin. It is a weak signal, so the timeout is generous.
  useEffect(() => {
    if (urlState !== 'ready' || revision < 1 || frameLoaded) return;
    const timer = setTimeout(() => raiseSoftFailure('load-timeout'), LOAD_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [urlState, revision, frameLoaded, raiseSoftFailure]);

  /**
   * The frame's origin is opaque, so `event.origin` is the string "null" and
   * cannot identify the sender. Identity comes from the WindowProxy instead —
   * comparing against `contentWindow` is legal cross-origin, reading it is not.
   * Only `lifecycle:error` exists today (bridge SDK); an automatic
   * `window.onerror` relay would have to be injected host-side into /drafts.
   */
  useEffect(() => {
    const handleFrameMessage = (event: MessageEvent) => {
      const frame = iframeRef.current;
      if (!frame || event.source !== frame.contentWindow) return;
      const data = event.data as { type?: string; info?: unknown } | null;
      if (!data || typeof data !== 'object' || data.type !== 'lifecycle:error') return;
      const detail = typeof data.info === 'string' ? data.info : undefined;
      raiseSoftFailure('runtime-error', detail);
    };
    window.addEventListener('message', handleFrameMessage);
    return () => window.removeEventListener('message', handleFrameMessage);
  }, [raiseSoftFailure]);

  const handleRetry = useCallback(() => {
    setSoftFailure(null);
    void resolveUrl();
    onRetry?.();
  }, [onRetry, resolveUrl]);

  const phase = resolvePreviewPhase({ draft, urlState, frameLoaded, generating });
  // The URL itself is stable across generate/undo, so the revision has to ride
  // along as a query param (stripped by `sanitize_path`) to defeat the HTTP
  // cache; the matching `key` forces a fresh frame rather than a re-navigation.
  const frameUrl = baseUrl && revision >= 1 ? `${baseUrl}?rev=${revision}` : null;
  const showFrame = frameUrl !== null && phase !== 'no-draft' && phase !== 'empty';

  return (
    <div
      className="relative h-full w-full overflow-hidden rounded-lg border border-[var(--border-subtle)] bg-[var(--background)]"
      role="region"
      aria-label={t(locale, 'creative.preview.title')}
      data-draft-preview={draftId ?? ''}
    >
      {showFrame && (
        <iframe
          key={frameKey}
          ref={iframeRef}
          sandbox={IFRAME_SANDBOX}
          src={frameUrl}
          title={t(locale, 'creative.preview.title')}
          className="h-full w-full border-0"
          onLoad={() => {
            assertSecureSandbox(IFRAME_SANDBOX, `DraftPreview(${draftId})`);
            setFrameLoaded(true);
          }}
          // Fires only for transport-level failures; an HTTP 4xx/5xx body still
          // counts as a successful load, which is part of why L2 stays fuzzy.
          onError={() => raiseSoftFailure('load-error')}
        />
      )}

      {phase === 'no-draft' && (
        <Centered>
          <p className="text-sm text-[var(--text)]">{t(locale, 'creative.preview.empty.title')}</p>
          <p className="text-xs text-[var(--text-secondary)]">{t(locale, 'creative.preview.empty.hint')}</p>
        </Centered>
      )}

      {phase === 'empty' && (
        <Centered>
          <p className="text-sm text-[var(--text)]">{t(locale, 'creative.preview.noRevision.title')}</p>
          <p className="text-xs text-[var(--text-secondary)]">{t(locale, 'creative.preview.noRevision.hint')}</p>
        </Centered>
      )}

      {/* No percentage here on purpose: the host has no real progress source. */}
      {(phase === 'generating' || phase === 'publishing') && (
        <Centered>
          <MathCurveLoader size={48} />
          <p className="text-sm text-[var(--text)]">
            {t(locale, phase === 'generating' ? 'creative.preview.generating' : 'creative.preview.publishing')}
          </p>
          <p className="text-xs text-[var(--text-secondary)]">
            {t(locale, phase === 'generating' ? 'creative.preview.generatingHint' : 'creative.preview.publishingHint')}
          </p>
        </Centered>
      )}

      {phase === 'port-unavailable' && (
        <Centered>
          <p className="text-sm text-[var(--text)]">{t(locale, 'creative.preview.notReady.title')}</p>
          <p className="text-xs text-[var(--text-secondary)]">{t(locale, 'creative.preview.notReady.hint')}</p>
          <button type="button" className="btn btn-primary" onClick={handleRetry}>
            <RefreshCw size={14} />
            {t(locale, 'creative.preview.retry')}
          </button>
        </Centered>
      )}

      {/* A raised timeout ends the wait: keeping the spinner would imply progress. */}
      {phase === 'loading' && !softFailure && (
        <Centered>
          <MathCurveLoader size={48} />
          <p className="text-xs text-[var(--text-secondary)]">{t(locale, 'creative.preview.loading')}</p>
        </Centered>
      )}

      {/* The revision is already on disk (section 10 L2) — warn and offer undo, never block. */}
      {softFailure && (phase === 'ready' || phase === 'loading') && (
        <div
          role="alert"
          className="absolute inset-x-0 bottom-0 flex items-start gap-2 border-t border-[var(--warning)] bg-[var(--surface)] px-3 py-2"
          data-soft-failure={softFailure.reason}
        >
          <AlertTriangle size={14} className="mt-0.5 shrink-0 text-[var(--warning)]" />
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium text-[var(--text)]">
              {t(locale, 'creative.preview.softFailure.title')}
            </div>
            <div className="text-[11px] text-[var(--text-secondary)]">
              {softFailure.detail ?? t(locale, 'creative.preview.softFailure.hint')}
            </div>
          </div>
          {onUndo && actions?.canUndo && (
            <button type="button" className="btn btn-primary shrink-0" onClick={onUndo}>
              <Undo2 size={14} />
              {t(locale, 'creative.preview.undo')}
            </button>
          )}
          <button
            type="button"
            className="shrink-0 text-[var(--text-secondary)] hover:text-[var(--text)]"
            aria-label={t(locale, 'creative.preview.dismiss')}
            onClick={() => setSoftFailure(null)}
          >
            <X size={14} />
          </button>
        </div>
      )}
    </div>
  );
}
