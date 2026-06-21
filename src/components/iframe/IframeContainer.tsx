'use client';

import { startTransition, useEffect, useRef, useState, useCallback } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { IFRAME_SANDBOX, assertSecureSandbox } from '@/lib/iframe-manager';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';

interface IframeContainerProps {
  moduleId: string;
  url: string;
  isVisible: boolean;
  onReady?: (moduleId: string) => void;
  onError?: (moduleId: string, error: string) => void;
}

const MAX_RETRIES = 3;

export default function IframeContainer({ moduleId, url, isVisible, onReady, onError }: IframeContainerProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [retryCount, setRetryCount] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const cleanupRef = useRef<(() => void) | null>(null);

  const loadIframe = useCallback(() => {
    setLoading(true);
    setError(null);
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadIframe();
  }, [url, loadIframe]);

  const handleLoad = () => {
    assertSecureSandbox(IFRAME_SANDBOX, `IframeContainer(${moduleId})`);
    setLoading(false);
    setRetryCount(0);
    onReady?.(moduleId);
  };

  const handleError = () => {
    setLoading(false);
    if (retryCount < MAX_RETRIES) {
      setRetryCount((c) => c + 1);
      setTimeout(() => loadIframe(), 1000 * (retryCount + 1));
    } else {
      const errMsg = `Failed to load module: ${moduleId}`;
      setError(errMsg);
      onError?.(moduleId, errMsg);
    }
  };

  const handleRetry = () => {
    setRetryCount(0);
    setError(null);
    loadIframe();
  };

  // Cleanup iframe on unmount
  useEffect(() => {
    return () => {
      cleanupRef.current?.();
      const iframe = iframeRef.current;
      if (iframe) {
        iframe.src = 'about:blank';
        iframe.remove();
      }
      cleanupRef.current = null;
    };
  }, []);

  if (error) {
    return (
      <div
        role="alert"
        aria-live="assertive"
        style={{
          display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
          height: '100%', padding: '2.5rem', textAlign: 'center',
        }}
      >
        <div style={{ fontSize: '2.75rem', marginBottom: '1rem' }}>&#9888;&#65039;</div>
        <p style={{ fontSize: '0.875rem', color: 'var(--vibe-brand-text)', marginBottom: '0.5rem' }}>{error}</p>
        <button className="btn btn-primary" onClick={handleRetry}>
          Retry
        </button>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      style={{
        width: '100%',
        height: '100%',
        position: 'relative',
        display: isVisible ? 'block' : 'none',
      }}
      role="region"
      aria-label={`Module: ${moduleId}`}
    >
      {loading && (
        <div style={{
          position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column', alignItems: 'center',
          justifyContent: 'center', color: 'var(--text-dim)', fontSize: FONT_SIZE.md,
          padding: 30, gap: 16
        }}>
          <MathCurveLoader size={60} />
          {retryCount > 0 && (
            <div style={{ fontSize: FONT_SIZE.sm }}>
              retry {retryCount}/{MAX_RETRIES}
            </div>
          )}
        </div>
      )}
      <iframe
        ref={iframeRef}
        sandbox={IFRAME_SANDBOX}
        src={url}
        onLoad={handleLoad}
        onError={handleError}
        style={{
          width: '100%',
          height: '100%',
          border: 'none',
          display: loading ? 'none' : 'block',
        }}
        title={moduleId}
      />
    </div>
  );
}
