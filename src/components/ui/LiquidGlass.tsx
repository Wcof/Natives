'use client';

import { useEffect, useRef, useCallback } from 'react';
import type { ReactNode } from 'react';
import { useCanvasQuota } from '@/context/ThemeContext';

// ── Types ──

interface LiquidGlassProps {
  isActive: boolean;
  children: ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

// ── Vertex shader: simple quad ──

const VERTEX_SHADER_SRC = `#version 300 es
in vec2 a_position;
in vec2 a_texCoord;
out vec2 v_texCoord;

void main() {
  gl_Position = vec4(a_position, 0.0, 1.0);
  v_texCoord = a_texCoord;
}
`;

// ── Fragment shader: fBm noise liquid refraction ──

const FRAGMENT_SHADER_SRC = `#version 300 es
precision highp float;

in vec2 v_texCoord;
out vec4 fragColor;

uniform float u_time;
uniform vec2 u_resolution;
uniform float u_frost;
uniform float u_refraction;

// --- Simplex-like noise helpers ---
vec3 mod289(vec3 x) { return x - floor(x * (1.0/289.0)) * 289.0; }
vec4 mod289(vec4 x) { return x - floor(x * (1.0/289.0)) * 289.0; }
vec4 permute(vec4 x) { return mod289(((x*34.0)+1.0)*x); }
vec4 taylorInvSqrt(vec4 r) { return 1.79284291400159 - 0.85373472095314 * r; }

float snoise(vec3 v) {
  const vec2 C = vec2(1.0/6.0, 1.0/3.0);
  const vec4 D = vec4(0.0, 0.5, 1.0, 2.0);

  vec3 i  = floor(v + dot(v, C.yyy));
  vec3 x0 = v - i + dot(i, C.xxx);

  vec3 g = step(x0.yzx, x0.xyz);
  vec3 l = 1.0 - g;
  vec3 i1 = min(g.xyz, l.zxy);
  vec3 i2 = max(g.xyz, l.zxy);

  vec3 x1 = x0 - i1 + C.xxx;
  vec3 x2 = x0 - i2 + C.yyy;
  vec3 x3 = x0 - D.yyy;

  i = mod289(i);
  vec4 p = permute(permute(permute(
    i.z + vec4(0.0, i1.z, i2.z, 1.0))
    + i.y + vec4(0.0, i1.y, i2.y, 1.0))
    + i.x + vec4(0.0, i1.x, i2.x, 1.0));

  float n_ = 0.142857142857;
  vec3 ns = n_ * D.wyz - D.xzx;

  vec4 j = p - 49.0 * floor(p * 7.0 * (1.0/49.0));

  vec4 x_ = floor(j * 7.0);
  vec4 y_ = floor(j - 7.0 * x_);

  vec4 x = x_ * ns.x + ns.yyyy;
  vec4 y = y_ * ns.x + ns.yyyy;
  vec4 h = 1.0 - abs(x) - abs(y);

  vec4 b0 = vec4(x.xy, y.xy);
  vec4 b1 = vec4(x.zw, y.zw);

  vec4 s0 = floor(b0) * 2.0 + 1.0;
  vec4 s1 = floor(b1) * 2.0 + 1.0;
  vec4 sh = -step(h, vec4(0.0));

  vec4 a0 = b0.xzyw + s0.xzyw * sh.xxyy;
  vec4 a1 = b1.xzyw + s1.xzyw * sh.zzww;

  vec3 p0 = vec3(a0.xy, h.x);
  vec3 p1 = vec3(a0.zw, h.y);
  vec3 p2 = vec3(a1.xy, h.z);
  vec3 p3 = vec3(a1.zw, h.w);

  vec4 norm = taylorInvSqrt(vec4(dot(p0,p0), dot(p1,p1), dot(p2,p2), dot(p3,p3)));
  p0 *= norm.x;
  p1 *= norm.y;
  p2 *= norm.z;
  p3 *= norm.w;

  vec4 m = max(0.6 - vec4(dot(x0,x0), dot(x1,x1), dot(x2,x2), dot(x3,x3)), 0.0);
  m = m * m;
  return 42.0 * dot(m*m, vec4(dot(p0,x0), dot(p1,x1), dot(p2,x2), dot(p3,x3)));
}

// fBm (fractal Brownian motion)
float fbm(vec3 p) {
  float value = 0.0;
  float amplitude = 0.5;
  float frequency = 1.0;
  for (int i = 0; i < 5; i++) {
    value += amplitude * snoise(p * frequency);
    frequency *= 2.0;
    amplitude *= 0.5;
  }
  return value;
}

void main() {
  vec2 uv = gl_FragCoord.xy / u_resolution;
  float aspect = u_resolution.x / u_resolution.y;
  vec2 pos = uv * 2.0 - 1.0;
  pos.x *= aspect;

  // Time-based slow drift
  float t = u_time * 0.15;

  // fBm noise for liquid distortion
  float noise1 = fbm(vec3(pos * 1.6, t));
  float noise2 = fbm(vec3(pos * 2.4 + 1.2, t + 2.0));

  // Refraction displacement
  vec2 refract = vec2(noise1, noise2) * u_refraction;
  vec2 distortedUV = uv + refract;

  // Edge glow: distance from center with noise perturbation
  vec2 centered = distortedUV * 2.0 - 1.0;
  centered.x *= aspect;
  float edgeDist = length(centered);
  float edgeGlow = smoothstep(0.9, 0.3, edgeDist) * 0.35;
  float edgeHighlight = smoothstep(1.0, 0.6, edgeDist) * 0.12;

  // Frost pattern
  float frost = u_frost;
  float frostPattern = fbm(vec3(distortedUV * 4.0, t * 1.2));
  frost = mix(frost, frost * frostPattern * 0.6, 0.3);

  // Base and glow colors
  vec3 baseColor = vec3(0.149, 0.161, 0.125);  // #262920
  vec3 glowColor = vec3(0.949, 1.0, 0.824);     // #F2FFD2

  // Combine
  vec3 color = baseColor;
  color += glowColor * edgeGlow * 0.5;
  color += glowColor * edgeHighlight * 0.3;

  // Subtle frost highlight
  color += glowColor * frost * 0.08 * (0.5 + 0.5 * frostPattern);

  // Final alpha with frost transparency
  float alpha = 0.85 + frost * 0.1;

  fragColor = vec4(color, alpha);
}
`;

// ── ActiveGlass: WebGL canvas + content ──

function ActiveGlass({ children, className, style }: { children: ReactNode; className?: string; style?: React.CSSProperties }) {
  const { allowed, release } = useCanvasQuota();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const glRef = useRef<WebGL2RenderingContext | null>(null);
  const animFrameRef = useRef<number>(0);
  const startTimeRef = useRef<number>(0);

  // CSS fallback when canvas quota is exceeded or WebGL unavailable
  if (!allowed) {
    return (
      <div
        ref={containerRef}
        className={className}
        style={{
          position: 'relative',
          background: 'rgba(255,255,255,0.1)',
          mixBlendMode: 'plus-lighter',
          border: '1px solid rgba(255,255,255,0.25)',
          boxShadow: '0 0 0 0.5px rgba(0,0,0,0.12), 0 12px 40px rgba(0,0,0,0.22)',
          color: '#F2FFD2',
          fontWeight: 600,
          ...style,
        }}
      >
        {children}
      </div>
    );
  }

  // WebGL initialization
  const initGL = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return false;

    const gl = canvas.getContext('webgl2', {
      alpha: true,
      premultipliedAlpha: false,
      antialias: false,
    });

    if (!gl) {
      // WebGL2 not supported, silently degrade
      return false;
    }

    glRef.current = gl;

    // Enable alpha blending
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    // Compile vertex shader
    const vertexShader = gl.createShader(gl.VERTEX_SHADER);
    if (!vertexShader) return false;
    gl.shaderSource(vertexShader, VERTEX_SHADER_SRC);
    gl.compileShader(vertexShader);
    if (!gl.getShaderParameter(vertexShader, gl.COMPILE_STATUS)) {
      gl.deleteShader(vertexShader);
      return false;
    }

    // Compile fragment shader
    const fragmentShader = gl.createShader(gl.FRAGMENT_SHADER);
    if (!fragmentShader) return false;
    gl.shaderSource(fragmentShader, FRAGMENT_SHADER_SRC);
    gl.compileShader(fragmentShader);
    if (!gl.getShaderParameter(fragmentShader, gl.COMPILE_STATUS)) {
      gl.deleteShader(vertexShader);
      gl.deleteShader(fragmentShader);
      return false;
    }

    // Create program
    const program = gl.createProgram();
    if (!program) return false;
    gl.attachShader(program, vertexShader);
    gl.attachShader(program, fragmentShader);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      gl.deleteProgram(program);
      gl.deleteShader(vertexShader);
      gl.deleteShader(fragmentShader);
      return false;
    }

    gl.useProgram(program);

    // Full-screen quad: two triangles covering clip space
    const positions = new Float32Array([
      -1, -1,
       1, -1,
      -1,  1,
      -1,  1,
       1, -1,
       1,  1,
    ]);
    const texCoords = new Float32Array([
      0, 0,
      1, 0,
      0, 1,
      0, 1,
      1, 0,
      1, 1,
    ]);

    const posBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, posBuf);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);

    const aPosition = gl.getAttribLocation(program, 'a_position');
    gl.enableVertexAttribArray(aPosition);
    gl.vertexAttribPointer(aPosition, 2, gl.FLOAT, false, 0, 0);

    const texBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, texBuf);
    gl.bufferData(gl.ARRAY_BUFFER, texCoords, gl.STATIC_DRAW);

    const aTexCoord = gl.getAttribLocation(program, 'a_texCoord');
    gl.enableVertexAttribArray(aTexCoord);
    gl.vertexAttribPointer(aTexCoord, 2, gl.FLOAT, false, 0, 0);

    // Store uniform locations
    const programData = {
      program,
      u_time: gl.getUniformLocation(program, 'u_time'),
      u_resolution: gl.getUniformLocation(program, 'u_resolution'),
      u_frost: gl.getUniformLocation(program, 'u_frost'),
      u_refraction: gl.getUniformLocation(program, 'u_refraction'),
    };

    // Store on canvas for render loop
    (canvas as any).__programData = programData;

    return true;
  }, []);

  // Resize handler
  const resizeCanvas = useCallback(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) return;

    const rect = container.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const width = Math.round(rect.width * dpr);
    const height = Math.round(rect.height * dpr);

    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
      canvas.style.width = `${rect.width}px`;
      canvas.style.height = `${rect.height}px`;
    }
  }, []);

  // Render loop
  const render = useCallback((timestamp: number) => {
    const gl = glRef.current;
    const canvas = canvasRef.current;
    if (!gl || !canvas) return;

    if (startTimeRef.current === 0) startTimeRef.current = timestamp;
    const elapsed = (timestamp - startTimeRef.current) / 1000;

    const pd = (canvas as any).__programData;
    if (!pd) return;

    gl.useProgram(pd.program);
    gl.uniform1f(pd.u_time, elapsed);
    gl.uniform2f(pd.u_resolution, canvas.width, canvas.height);
    gl.uniform1f(pd.u_frost, 0.48);
    gl.uniform1f(pd.u_refraction, 0.028);

    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    animFrameRef.current = requestAnimationFrame(render);
  }, []);

  // Setup and teardown
  useEffect(() => {
    if (!allowed) return;

    const canvas = canvasRef.current;
    if (!canvas) return;

    const ok = initGL();
    if (!ok) {
      release();
      return;
    }

    resizeCanvas();

    // Observe resize
    const container = containerRef.current;
    let resizeObserver: ResizeObserver | null = null;
    if (container) {
      resizeObserver = new ResizeObserver(() => {
        resizeCanvas();
      });
      resizeObserver.observe(container);
    }

    // Start render loop
    startTimeRef.current = 0;
    animFrameRef.current = requestAnimationFrame(render);

    return () => {
      cancelAnimationFrame(animFrameRef.current);

      if (resizeObserver) {
        resizeObserver.disconnect();
      }

      const gl = glRef.current;
      if (gl) {
        const ext = gl.getExtension('WEBGL_lose_context');
        if (ext) ext.loseContext();
        glRef.current = null;
      }
    };
  }, [allowed, initGL, release, render, resizeCanvas]);

  return (
    <div
      ref={containerRef}
      className={className}
      style={{
        position: 'relative',
        ...style,
      }}
    >
      <canvas
        ref={canvasRef}
        style={{
          position: 'absolute',
          inset: 0,
          width: '100%',
          height: '100%',
          pointerEvents: 'none',
          display: 'block',
        }}
      />
      <div style={{ position: 'relative', zIndex: 1 }}>
        {children}
      </div>
    </div>
  );
}

// ── LiquidGlass component ──

export default function LiquidGlass({ isActive, children, className, style }: LiquidGlassProps) {
  if (!isActive) {
    return (
      <div className={className} style={style}>
        {children}
      </div>
    );
  }

  return (
    <ActiveGlass className={className} style={style}>
      {children}
    </ActiveGlass>
  );
}
