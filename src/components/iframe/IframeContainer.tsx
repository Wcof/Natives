'use client';

import { useEffect, useRef, useState, useCallback } from 'react';

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
    loadIframe();
  }, [url, loadIframe]);

  const handleLoad = () => {
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
          height: '100%', padding: 40, textAlign: 'center',
        }}
      >
        <div style={{ fontSize: 44, marginBottom: 16 }}>&#9888;&#65039;</div>
        <p style={{ fontSize: 14, color: 'var(--text,#f2f2ea)', marginBottom: 8 }}>{error}</p>
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
          position: 'absolute', inset: 0, display: 'flex', alignItems: 'center',
          justifyContent: 'center', color: 'var(--text-faint,#62655a)', fontSize: 13,
          padding: 30,
        }}>
          Loading...{retryCount > 0 ? ` (retry ${retryCount}/${MAX_RETRIES})` : ''}
        </div>
      )}
      <iframe
        ref={iframeRef}
        sandbox="allow-scripts allow-forms"
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
