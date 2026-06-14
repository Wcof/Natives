'use client';

import { useEffect, useRef, useState } from 'react';

interface IframeContainerProps {
  moduleId: string;
  url: string;
  isVisible: boolean;
  onReady?: (moduleId: string) => void;
  onError?: (moduleId: string, error: string) => void;
}

export default function IframeContainer({ moduleId, url, isVisible, onReady, onError }: IframeContainerProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setLoading(true);
    setError(null);
  }, [url]);

  const handleLoad = () => {
    setLoading(false);
    onReady?.(moduleId);
  };

  const handleError = () => {
    setLoading(false);
    const errMsg = `Failed to load module: ${moduleId}`;
    setError(errMsg);
    onError?.(moduleId, errMsg);
  };

  if (error) {
    return (
      <div style={{
        display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
        height: '100%', padding: 40, textAlign: 'center',
      }}>
        <div style={{ fontSize: 44, marginBottom: 16 }}>⚠️</div>
        <p style={{ fontSize: 14, color: 'var(--text,#f2f2ea)', marginBottom: 8 }}>{error}</p>
        <button className="btn btn-primary" onClick={() => window.location.reload()}>
          Reload
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
          Loading...
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
