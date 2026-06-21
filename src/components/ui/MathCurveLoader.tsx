'use client';

import { useEffect, useRef } from 'react';

interface MathCurveLoaderProps {
  size?: number;
  strokeWidth?: number;
  particleCount?: number;
  style?: React.CSSProperties;
}

export function MathCurveLoader({
  size = 72,
  strokeWidth = 2.2,
  particleCount = 18,
  style,
}: MathCurveLoaderProps) {
  const containerRef = useRef<SVGSVGElement>(null);
  const pathRef = useRef<SVGPathElement>(null);
  const groupRef = useRef<SVGGElement>(null);
  const particlesRef = useRef<SVGCircleElement[]>([]);

  // Configuration for Lissajous Curve (Oscilloscope / AI Thinking representation)
  const config = {
    durationMs: 4600,
    pulseDurationMs: 3800,
    rotationDurationMs: 24000,
    trailSpan: 0.35,
    lissajousAmp: 25,
    lissajousAmpBoost: 5,
    lissajousAX: 3,
    lissajousBY: 4,
    lissajousPhase: 1.57,
    lissajousYScale: 0.92,
  };

  useEffect(() => {
    let animationId: number;
    const startTime = performance.now();

    const getPoint = (p: number, detailScale: number) => {
      const t = p * Math.PI * 2;
      const amp = config.lissajousAmp + detailScale * config.lissajousAmpBoost;
      return {
        x: 50 + Math.sin(config.lissajousAX * t + config.lissajousPhase) * amp,
        y: 50 + Math.sin(config.lissajousBY * t) * (amp * config.lissajousYScale),
      };
    };

    const tick = (now: number) => {
      const time = now - startTime;

      // 1. Calculate detail breathing scale
      const pulseProgress = (time % config.pulseDurationMs) / config.pulseDurationMs;
      const pulseAngle = pulseProgress * Math.PI * 2;
      const detailScale = 0.52 + ((Math.sin(pulseAngle + 0.55) + 1) / 2) * 0.48;

      // 2. Calculate rotation
      const rotation = -((time % config.rotationDurationMs) / config.rotationDurationMs) * 360;

      // 3. Update background path
      if (pathRef.current) {
        const steps = 120;
        let d = '';
        for (let i = 0; i <= steps; i++) {
          const pt = getPoint(i / steps, detailScale);
          d += `${i === 0 ? 'M' : 'L'} ${pt.x.toFixed(2)} ${pt.y.toFixed(2)}`;
        }
        pathRef.current.setAttribute('d', d);
      }

      // 4. Update rotation group
      if (groupRef.current) {
        groupRef.current.setAttribute('transform', `rotate(${rotation.toFixed(2)} 50 50)`);
      }

      // 5. Update flowing particles
      const progress = (time % config.durationMs) / config.durationMs;
      particlesRef.current.forEach((node, index) => {
        if (!node) return;
        const tailOffset = index / (particleCount - 1);
        let p = progress - tailOffset * config.trailSpan;
        p = ((p % 1) + 1) % 1; // Normalize to [0, 1]

        const pt = getPoint(p, detailScale);
        const fade = Math.pow(1 - tailOffset, 0.6);

        node.setAttribute('cx', pt.x.toFixed(2));
        node.setAttribute('cy', pt.y.toFixed(2));
        node.setAttribute('r', (0.7 + fade * 1.5).toFixed(2));
        node.setAttribute('opacity', (0.05 + fade * 0.95).toFixed(3));
      });

      animationId = requestAnimationFrame(tick);
    };

    animationId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(animationId);
  }, [particleCount]);

  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: size,
        height: size,
        ...style,
      }}
    >
      <svg
        ref={containerRef}
        viewBox="0 0 100 100"
        fill="none"
        style={{
          width: '100%',
          height: '100%',
          overflow: 'visible',
        }}
      >
        <defs>
          {/* Glowing neon filter */}
          <filter id="glow-filter" x="-30%" y="-30%" width="160%" height="160%">
            <feGaussianBlur stdDeviation="1.6" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>

          {/* Holographic Gradient matching the AI Accent theme */}
          <linearGradient id="curve-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stopColor="var(--accent)" />
            <stop offset="50%" stopColor="var(--accent-soft)" />
            <stop offset="100%" stopColor="color-mix(in srgb, var(--accent) 30%, #4f46e5)" />
          </linearGradient>
        </defs>

        <g ref={groupRef}>
          {/* Underlay shadow path */}
          <path
            ref={pathRef}
            stroke="url(#curve-gradient)"
            strokeWidth={strokeWidth}
            strokeLinecap="round"
            strokeLinejoin="round"
            opacity="0.12"
          />

          {/* Flowing energy particles */}
          {Array.from({ length: particleCount }).map((_, index) => (
            <circle
              key={index}
              ref={(el) => {
                if (el) particlesRef.current[index] = el;
              }}
              fill="url(#curve-gradient)"
              filter="url(#glow-filter)"
            />
          ))}
        </g>
      </svg>
    </div>
  );
}
