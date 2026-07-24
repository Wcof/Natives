'use client';

interface MathCurveLoaderProps {
  size?: number;
  strokeWidth?: number;
  particleCount?: number;
  style?: React.CSSProperties;
}

export function MathCurveLoader({ size = 72, strokeWidth = 2.2, style }: MathCurveLoaderProps) {
  return (
    <div className="math-curve-loader" style={{ width: size, height: size, ...style }} aria-label="Loading">
      <svg viewBox="0 0 100 100" fill="none" width="100%" height="100%" aria-hidden="true">
        <circle cx="50" cy="50" r="35" stroke="var(--primary-soft)" strokeWidth={strokeWidth} opacity=".25" />
        <path d="M15 50 C28 12 42 88 55 50 S78 12 86 50" stroke="var(--primary)" strokeWidth={strokeWidth} strokeLinecap="round" />
      </svg>
    </div>
  );
}
