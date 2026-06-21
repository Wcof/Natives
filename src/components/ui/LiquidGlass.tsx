'use client';

import { useEffect, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { useCanvasQuota } from '@/context/ThemeContext';
import { useHydrated } from '@/hooks/useHydrated';

interface LiquidGlassProps {
  isActive: boolean;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  blurAmount?: number;
  displacementScale?: number;
  saturation?: number;
  aberrationIntensity?: number;
  elasticity?: number;
}

interface ShaderUniforms {
  time: WebGLUniformLocation | null;
  resolution: WebGLUniformLocation | null;
  frost: WebGLUniformLocation | null;
  refraction: WebGLUniformLocation | null;
  background: WebGLUniformLocation | null;
  borderColor: WebGLUniformLocation | null;
}

interface WebGLResources {
  program: WebGLProgram;
  vertexShader: WebGLShader;
  fragmentShader: WebGLShader;
  positionBuffer: WebGLBuffer;
  vertexArray: WebGLVertexArrayObject;
  uniforms: ShaderUniforms;
}

const LIQUID_GLASS_METRICS = {
  frost: 0.45,
  refraction: 0.026,
  background: [242 / 255, 1, 210 / 255, 0.12] as const,
  borderColor: [1, 1, 1, 0.28] as const,
};

const VERTEX_SHADER_SOURCE = `#version 300 es
in vec2 a_position;
out vec2 v_uv;

void main() {
  v_uv = a_position * 0.5 + 0.5;
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`;

const FRAGMENT_SHADER_SOURCE = `#version 300 es
precision highp float;

in vec2 v_uv;
out vec4 frag_color;

uniform float u_time;
uniform vec2 u_resolution;
uniform float u_frost;
uniform float u_refraction;
uniform vec4 u_background;
uniform vec4 u_border_color;

float hash21(vec2 point) {
  point = fract(point * vec2(123.34, 456.21));
  point += dot(point, point + 45.32);
  return fract(point.x * point.y);
}

float value_noise(vec2 point) {
  vec2 cell = floor(point);
  vec2 local = fract(point);
  local = local * local * (3.0 - 2.0 * local);

  float a = hash21(cell);
  float b = hash21(cell + vec2(1.0, 0.0));
  float c = hash21(cell + vec2(0.0, 1.0));
  float d = hash21(cell + vec2(1.0, 1.0));

  return mix(mix(a, b, local.x), mix(c, d, local.x), local.y);
}

float fbm(vec2 point) {
  float value = 0.0;
  float amplitude = 0.5;
  mat2 rotation = mat2(0.80, -0.60, 0.60, 0.80);

  for (int octave = 0; octave < 4; octave++) {
    value += value_noise(point) * amplitude;
    point = rotation * point * 2.03 + 17.17;
    amplitude *= 0.5;
  }

  return value;
}

void main() {
  vec2 resolution = max(u_resolution, vec2(1.0));
  float aspect = resolution.x / resolution.y;
  vec2 field = v_uv;
  field.x *= aspect;

  float drift = u_time * 0.12;
  float flow_x = fbm(field * 3.1 + vec2(drift, -drift * 0.65));
  float flow_y = fbm(field * 3.7 + vec2(-drift * 0.55, drift));
  vec2 refracted_uv = v_uv + (vec2(flow_x, flow_y) - 0.5) * u_refraction;

  float frost_noise = fbm(refracted_uv * vec2(8.0, 5.0) + drift * 0.35);
  float sheen = smoothstep(0.28, 0.92, frost_noise) * u_frost;

  float edge_distance = min(
    min(refracted_uv.x, 1.0 - refracted_uv.x),
    min(refracted_uv.y, 1.0 - refracted_uv.y)
  );
  float edge = 1.0 - smoothstep(0.0, 0.045, edge_distance);

  vec3 color = u_background.rgb;
  color += vec3(1.0) * sheen * 0.075;
  color = mix(color, u_border_color.rgb, edge * u_border_color.a * 0.72);

  float alpha = u_background.a + sheen * 0.035 + edge * u_border_color.a * 0.42;
  frag_color = vec4(color, clamp(alpha, 0.0, 0.42));
}
`;

function mergeClasses(
  ...classes: Array<string | false | null | undefined>
): string {
  return classes.filter(Boolean).join(' ');
}

function compileShader(
  gl: WebGL2RenderingContext,
  type: number,
  source: string,
): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) {
    return null;
  }

  gl.shaderSource(shader, source);
  gl.compileShader(shader);

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    console.error('[LiquidGlass] Shader compilation failed:', gl.getShaderInfoLog(shader));
    gl.deleteShader(shader);
    return null;
  }

  return shader;
}

function createWebGLResources(
  gl: WebGL2RenderingContext,
): WebGLResources | null {
  const vertexShader = compileShader(
    gl,
    gl.VERTEX_SHADER,
    VERTEX_SHADER_SOURCE,
  );
  if (!vertexShader) {
    return null;
  }

  const fragmentShader = compileShader(
    gl,
    gl.FRAGMENT_SHADER,
    FRAGMENT_SHADER_SOURCE,
  );
  if (!fragmentShader) {
    gl.deleteShader(vertexShader);
    return null;
  }

  const program = gl.createProgram();
  if (!program) {
    gl.deleteShader(vertexShader);
    gl.deleteShader(fragmentShader);
    return null;
  }

  gl.attachShader(program, vertexShader);
  gl.attachShader(program, fragmentShader);
  gl.linkProgram(program);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.error('[LiquidGlass] Program linking failed:', gl.getProgramInfoLog(program));
    gl.deleteProgram(program);
    gl.deleteShader(vertexShader);
    gl.deleteShader(fragmentShader);
    return null;
  }

  const positionBuffer = gl.createBuffer();
  const vertexArray = gl.createVertexArray();
  if (!positionBuffer || !vertexArray) {
    if (positionBuffer) {
      gl.deleteBuffer(positionBuffer);
    }
    if (vertexArray) {
      gl.deleteVertexArray(vertexArray);
    }
    gl.deleteProgram(program);
    gl.deleteShader(vertexShader);
    gl.deleteShader(fragmentShader);
    return null;
  }

  gl.bindVertexArray(vertexArray);
  gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([
      -1, -1,
      1, -1,
      -1, 1,
      -1, 1,
      1, -1,
      1, 1,
    ]),
    gl.STATIC_DRAW,
  );

  const positionLocation = gl.getAttribLocation(program, 'a_position');
  if (positionLocation < 0) {
    gl.deleteBuffer(positionBuffer);
    gl.deleteVertexArray(vertexArray);
    gl.deleteProgram(program);
    gl.deleteShader(vertexShader);
    gl.deleteShader(fragmentShader);
    return null;
  }

  gl.enableVertexAttribArray(positionLocation);
  gl.vertexAttribPointer(positionLocation, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);
  gl.bindBuffer(gl.ARRAY_BUFFER, null);

  return {
    program,
    vertexShader,
    fragmentShader,
    positionBuffer,
    vertexArray,
    uniforms: {
      time: gl.getUniformLocation(program, 'u_time'),
      resolution: gl.getUniformLocation(program, 'u_resolution'),
      frost: gl.getUniformLocation(program, 'u_frost'),
      refraction: gl.getUniformLocation(program, 'u_refraction'),
      background: gl.getUniformLocation(program, 'u_background'),
      borderColor: gl.getUniformLocation(program, 'u_border_color'),
    },
  };
}

function destroyWebGLResources(
  gl: WebGL2RenderingContext,
  resources: WebGLResources,
): void {
  gl.deleteBuffer(resources.positionBuffer);
  gl.deleteVertexArray(resources.vertexArray);
  gl.deleteProgram(resources.program);
  gl.deleteShader(resources.vertexShader);
  gl.deleteShader(resources.fragmentShader);
}

function ActiveLiquidGlass({
  children,
  className,
  style,
  blurAmount = 0.40,
  displacementScale = 64,
  saturation = 135,
  aberrationIntensity = 2,
  elasticity = 0,
}: Omit<LiquidGlassProps, 'isActive'>) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const blurAmountRef = useRef(blurAmount);
  const displacementScaleRef = useRef(displacementScale);
  const saturationRef = useRef(saturation);
  const aberrationIntensityRef = useRef(aberrationIntensity);
  const elasticityRef = useRef(elasticity);
  const mounted = useHydrated();
  const [webGLUnavailable, setWebGLUnavailable] = useState(false);
  const { allowed, release } = useCanvasQuota(mounted && !webGLUnavailable);

  

  useEffect(() => {
    blurAmountRef.current = blurAmount;
    displacementScaleRef.current = displacementScale;
    saturationRef.current = saturation;
    aberrationIntensityRef.current = aberrationIntensity;
    elasticityRef.current = elasticity;
  }, [blurAmount, displacementScale, saturation, aberrationIntensity, elasticity]);

  useEffect(() => {
    if (!allowed || webGLUnavailable) {
      return;
    }

    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) {
      return;
    }

    const gl = canvas.getContext('webgl2', {
      alpha: true,
      antialias: false,
      depth: false,
      desynchronized: true,
      failIfMajorPerformanceCaveat: true,
      powerPreference: 'high-performance',
      premultipliedAlpha: true,
      preserveDrawingBuffer: false,
      stencil: false,
    });

    if (!gl) {
      setWebGLUnavailable(true);
      release();
      return;
    }

    const resources = createWebGLResources(gl);
    if (!resources) {
      setWebGLUnavailable(true);
      release();
      return;
    }

    let animationFrame = 0;
    let startTime = 0;
    let isVisible = true;
    const prefersReducedMotion = window.matchMedia(
      '(prefers-reduced-motion: reduce)',
    ).matches;

    const resizeCanvas = () => {
      const bounds = container.getBoundingClientRect();
      const deviceScale = Math.min(window.devicePixelRatio || 1, 1.5);
      const width = Math.max(1, Math.round(bounds.width * deviceScale));
      const height = Math.max(1, Math.round(bounds.height * deviceScale));

      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
    };

    const draw = (timestamp: number) => {
      if (!isVisible || document.hidden || gl.isContextLost()) {
        animationFrame = 0;
        return;
      }

      if (startTime === 0) {
        startTime = timestamp;
      }

      resizeCanvas();

      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.useProgram(resources.program);
      gl.bindVertexArray(resources.vertexArray);

      gl.uniform1f(resources.uniforms.time, (timestamp - startTime) / 1000);
      gl.uniform2f(
        resources.uniforms.resolution,
        canvas.width,
        canvas.height,
      );
      gl.uniform1f(resources.uniforms.frost, blurAmountRef.current * 1.125);
      gl.uniform1f(
        resources.uniforms.refraction,
        displacementScaleRef.current / 2500,
      );
      gl.uniform4fv(
        resources.uniforms.background,
        LIQUID_GLASS_METRICS.background,
      );
      gl.uniform4fv(
        resources.uniforms.borderColor,
        LIQUID_GLASS_METRICS.borderColor,
      );
      gl.drawArrays(gl.TRIANGLES, 0, 6);
      gl.bindVertexArray(null);

      if (!prefersReducedMotion) {
        animationFrame = window.requestAnimationFrame(draw);
      } else {
        animationFrame = 0;
      }
    };

    const startRendering = () => {
      if (
        animationFrame === 0 &&
        isVisible &&
        !document.hidden &&
        !gl.isContextLost()
      ) {
        animationFrame = window.requestAnimationFrame(draw);
      }
    };

    const stopRendering = () => {
      if (animationFrame !== 0) {
        window.cancelAnimationFrame(animationFrame);
        animationFrame = 0;
      }
    };

    const handleVisibilityChange = () => {
      if (document.hidden) {
        stopRendering();
      } else {
        startRendering();
      }
    };

    const handleContextLost = (event: Event) => {
      event.preventDefault();
      stopRendering();
      setWebGLUnavailable(true);
      release();
    };

    const resizeObserver = new ResizeObserver(() => {
      resizeCanvas();
      startRendering();
    });
    resizeObserver.observe(container);

    const intersectionObserver = new IntersectionObserver((entries) => {
      isVisible = entries[0]?.isIntersecting ?? true;
      if (isVisible) {
        startRendering();
      } else {
        stopRendering();
      }
    });
    intersectionObserver.observe(container);

    document.addEventListener('visibilitychange', handleVisibilityChange);
    canvas.addEventListener('webglcontextlost', handleContextLost);
    startRendering();

    return () => {
      stopRendering();
      resizeObserver.disconnect();
      intersectionObserver.disconnect();
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      canvas.removeEventListener('webglcontextlost', handleContextLost);

      if (!gl.isContextLost()) {
        destroyWebGLResources(gl, resources);
      }
    };
  }, [allowed, release, webGLUnavailable]);

  const useCssFallback = !allowed || webGLUnavailable;

  return (
    <div
      ref={containerRef}
      className={mergeClasses(
        'relative isolate overflow-hidden text-brand-jade-glow font-semibold',
        'animate-liquid-surface-in transition-transform duration-[400ms] ease-ease-out-expo',
        useCssFallback
          ? 'border border-white/25 bg-white/10 mix-blend-plus-lighter shadow-liquid-edge'
          : 'border border-glass-edge/[0.28] bg-glass-active/[0.12] shadow-liquid-edge shadow-glass-ambient',
        className,
      )}
      style={style}
      data-liquid-glass={useCssFallback ? 'css-fallback' : 'webgl'}
    >
      {!useCssFallback && (
        <canvas
          ref={canvasRef}
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 block h-full w-full"
        />
      )}
      <div className="relative z-10 h-full w-full text-inherit" data-liquid-ignore>
        {children}
      </div>
    </div>
  );
}

export default function LiquidGlass({
  isActive,
  children,
  className,
  style,
  blurAmount,
  displacementScale,
  saturation,
  aberrationIntensity,
  elasticity,
}: LiquidGlassProps) {
  if (!isActive) {
    return (
      <div
        className={mergeClasses(
          'bg-brand-olive-sidebar transition-all duration-[400ms] ease-ease-out-expo hover:bg-white/[0.04]',
          className,
        )}
        style={style}
      >
        {children}
      </div>
    );
  }

  return (
    <ActiveLiquidGlass
      className={className}
      style={style}
      blurAmount={blurAmount}
      displacementScale={displacementScale}
      saturation={saturation}
      aberrationIntensity={aberrationIntensity}
      elasticity={elasticity}
    >
      {children}
    </ActiveLiquidGlass>
  );
}

