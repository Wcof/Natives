'use client';

import { useEffect, useCallback, useRef } from 'react';
import { FONT_SIZE } from '@/lib/design-tokens';

interface ImageLightboxProps {
  src: string;
  alt?: string;
  onClose: () => void;
}

export default function ImageLightbox({ src, alt, onClose }: ImageLightboxProps) {
  const scaleRef = useRef(1);
  const imgRef = useRef<HTMLImageElement>(null);

  const handleWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    scaleRef.current = Math.min(8, Math.max(0.2, scaleRef.current - e.deltaY * 0.002));
    if (imgRef.current) {
      imgRef.current.style.transform = `scale(${scaleRef.current})`;
    }
  }, []);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') onClose();
  }, [onClose]);

  useEffect(() => {
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  return (
    <div
      className="lightbox-overlay"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
      onWheel={handleWheel}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.85)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
        cursor: 'zoom-out',
      }}
    >
      <img
        ref={imgRef}
        src={src}
        alt={alt || ''}
        style={{
          maxWidth: '90vw',
          maxHeight: '90vh',
          objectFit: 'contain',
          transition: 'transform 0.1s ease-out',
        }}
      />
      <div style={{
        position: 'absolute',
        bottom: 20,
        left: '50%',
        transform: 'translateX(-50%)',
        color: 'rgba(255,255,255,0.5)',
        fontSize: FONT_SIZE.md,
        pointerEvents: 'none',
      }}>
        Click outside to close · Scroll to zoom
      </div>
    </div>
  );
}
