'use client';

import { useState, useEffect, useRef } from 'react';
import { t, useLocale } from '@/i18n';
import {
  Wifi,
  Bluetooth,
  Moon,
  Sun,
  Volume2,
  VolumeX,
  Activity,
  ShieldCheck,
  ShieldAlert,
  Layers,
} from 'lucide-react';
import { FONT_SIZE } from '@/lib/design-tokens';



export default function ControlHubWidget() {
  const locale = useLocale();
  const containerRef = useRef<HTMLDivElement>(null);

  // --- Widget State ---
  const [clicks, setClicks] = useState(0);
  const [cpuUsage, setCpuUsage] = useState(0);
  const [memoryUsedBytes, setMemoryUsedBytes] = useState(0);
  const [memoryTotalBytes, setMemoryTotalBytes] = useState(0);
  const [isSandboxActive, setIsSandboxActive] = useState(false);
  const [isWifiOn, setIsWifiOn] = useState(true);
  const [isBluetoothOn, setIsBluetoothOn] = useState(true);
  const [isAirdropOn, setIsAirdropOn] = useState(false);
  const [isDndOn, setIsDndOn] = useState(false);
  const [brightness, setBrightness] = useState(75);
  const [volume, setVolume] = useState(50);

  // --- Real System Metrics Polling (R-NO-FAKE-DATA) ---
  // Replaces previous Math.random() CPU simulation with real sysinfo backend.
  useEffect(() => {
    let cancelled = false;
    const api = window.nativesAPI;
    if (!api?.disk?.systemMetrics) return;

    const poll = async () => {
      try {
        const m = await api.disk.systemMetrics();
        if (cancelled) return;
        setCpuUsage(Math.round(m.cpuUsage));
        setMemoryUsedBytes(m.memoryUsedBytes);
        setMemoryTotalBytes(m.memoryTotalBytes);
      } catch (err) {
        console.warn('Failed to fetch system metrics:', err);
      }
    };

    const onVisibility = () => {
      if (document.visibilityState === 'visible') void poll();
    };
    onVisibility();
    document.addEventListener('visibilitychange', onVisibility);
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') void poll();
    }, 5000);
    return () => {
      cancelled = true;
      clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, []);

  // Window draggability helper styles (Tauri v2 uses data-tauri-drag-region attribute)
  const dragAttr = { 'data-tauri-drag-region': '' as string } as React.HTMLAttributes<HTMLDivElement>;

  return (
    <div
      ref={containerRef}
      className="select-none"
      style={{
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'center',
        width: '100%',
        height: '100%',
        minHeight: '100%',
        position: 'relative',
        overflow: 'hidden',
        fontFamily: 'var(--font-ui), system-ui, sans-serif',
        background: 'var(--background)',
      }}
    >
      {/* V1.0 已移除：wallpaper 渐变背景 + liquid-blob 动画层（纯色 Surface 体系） */}

      {/* Main Widget Card — V1.0 纯色 Surface + 轻边框 */}
      <div
        style={{
          width: 390,
          position: 'relative',
          zIndex: 10,
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-xl)',
          boxShadow: 'var(--shadow-modal)',
        }}
      >
        <div
          className="main-card flex flex-col text-white"
          data-tauri-drag-region
          style={{
            width: '390px',
            minHeight: '490px',
            padding: '24px',
            boxSizing: 'border-box',
          }}
        >
          {/* Header */}
          <div className="flex flex-col items-center justify-center text-center mb-5 mt-1 pb-1 border-b border-white/10">
            <h1
              className="font-bold tracking-tight text-shadow-md"
              style={{
                fontSize: FONT_SIZE.lg + 2,
                fontFamily: 'var(--font-display)',
                textShadow: '0 2px 8px rgba(0,0,0,0.35)',
              }}
            >
              {t(locale, 'controlHub.title')}
            </h1>
            <p
              className="text-white/60 mt-1"
              style={{
                fontSize: FONT_SIZE.xs,
                textShadow: '0 1px 3px rgba(0,0,0,0.2)',
              }}
            >
              {t(locale, 'controlHub.subtitle')}
            </p>
          </div>

          <div className="flex-1 flex flex-col justify-between" >
            <div className="flex flex-col gap-4 animate-fade-in">
                {/* Network & Connection Capsule Row */}
                <div className="grid grid-cols-3 gap-2">
                  <button
                    onClick={() => setIsWifiOn(!isWifiOn)}
                    className={`flex flex-col items-center justify-center p-3 rounded-2xl transition-all border ${
                      isWifiOn
                        ? 'bg-blue-600/60 border-blue-400/30 text-white shadow-lg'
                        : 'bg-white/5 border-white/5 text-white/50 hover:bg-white/10'
                    }`}
                  >
                    <Wifi size={18} className="mb-1.5" />
                    <span className="text-[10px] font-bold tracking-tight">{t(locale, 'controlHub.wifi')}</span>
                    <span className="text-[9px] opacity-75 mt-0.5">
                      {isWifiOn ? t(locale, 'controlHub.connected') : t(locale, 'controlHub.disconnected')}
                    </span>
                  </button>

                  <button
                    onClick={() => setIsBluetoothOn(!isBluetoothOn)}
                    className={`flex flex-col items-center justify-center p-3 rounded-2xl transition-all border ${
                      isBluetoothOn
                        ? 'bg-blue-600/60 border-blue-400/30 text-white shadow-lg'
                        : 'bg-white/5 border-white/5 text-white/50 hover:bg-white/10'
                    }`}
                  >
                    <Bluetooth size={18} className="mb-1.5" />
                    <span className="text-[10px] font-bold tracking-tight">{t(locale, 'controlHub.bluetooth')}</span>
                    <span className="text-[9px] opacity-75 mt-0.5">
                      {isBluetoothOn ? t(locale, 'controlHub.connected') : t(locale, 'controlHub.disconnected')}
                    </span>
                  </button>

                  <button
                    onClick={() => setIsAirdropOn(!isAirdropOn)}
                    className={`flex flex-col items-center justify-center p-3 rounded-2xl transition-all border ${
                      isAirdropOn
                        ? 'bg-blue-600/60 border-blue-400/30 text-white shadow-lg'
                        : 'bg-white/5 border-white/5 text-white/50 hover:bg-white/10'
                    }`}
                  >
                    <Layers size={18} className="mb-1.5" />
                    <span className="text-[10px] font-bold tracking-tight">{t(locale, 'controlHub.airdrop')}</span>
                    <span className="text-[9px] opacity-75 mt-0.5">
                      {isAirdropOn ? t(locale, 'controlHub.connected') : t(locale, 'controlHub.disconnected')}
                    </span>
                  </button>
                </div>

                {/* Focus / DND Group */}
                <button
                  onClick={() => setIsDndOn(!isDndOn)}
                  className={`flex items-center gap-3 w-full p-3.5 rounded-2xl border transition-all ${
                    isDndOn
                      ? 'bg-purple-600/40 border-purple-400/30 text-white shadow-lg shadow-purple-900/10'
                      : 'bg-white/5 border-white/5 text-white/80 hover:bg-white/10'
                  }`}
                >
                  <div className={`p-1.5 rounded-lg ${isDndOn ? 'bg-purple-500/30' : 'bg-white/10'}`}>
                    <Moon size={15} />
                  </div>
                  <div className="flex flex-col text-left">
                    <span className="text-xs font-semibold">{t(locale, 'controlHub.focusMode')}</span>
                    <span className="text-[10px] opacity-60">
                      {isDndOn ? t(locale, 'controlHub.dnd') : t(locale, 'controlHub.disconnected')}
                    </span>
                  </div>
                </button>

                {/* Sliders Container */}
                <div className="flex flex-col gap-3 p-4 bg-white/5 border border-white/5 rounded-2xl">
                  <div className="flex items-center gap-3">
                    <Sun size={15} className="text-white/60 shrink-0" />
                    <div className="flex-1 flex flex-col gap-1">
                      <div className="flex justify-between items-center text-[10px] text-white/60">
                        <span>{t(locale, 'controlHub.brightness')}</span>
                        <span className="font-mono">{brightness}%</span>
                      </div>
                      <input
                        type="range" min="0" max="100" value={brightness}
                        onChange={(e) => setBrightness(Number(e.target.value))}
                        className="w-full h-1.5 bg-white/10 rounded-lg appearance-none cursor-pointer accent-white transition-all hover:bg-white/20"
                      />
                    </div>
                  </div>

                  <div className="flex items-center gap-3 border-t border-white/5 pt-3">
                    {volume === 0 ? (
                      <VolumeX size={15} className="text-white/40 shrink-0" />
                    ) : (
                      <Volume2 size={15} className="text-white/60 shrink-0" />
                    )}
                    <div className="flex-1 flex flex-col gap-1">
                      <div className="flex justify-between items-center text-[10px] text-white/60">
                        <span>{t(locale, 'controlHub.volume')}</span>
                        <span className="font-mono">{volume}%</span>
                      </div>
                      <input
                        type="range" min="0" max="100" value={volume}
                        onChange={(e) => setVolume(Number(e.target.value))}
                        className="w-full h-1.5 bg-white/10 rounded-lg appearance-none cursor-pointer accent-white transition-all hover:bg-white/20"
                      />
                    </div>
                  </div>
                </div>

                {/* System Monitor Panel */}
                <div className="flex flex-col gap-3 p-4 bg-white/5 border border-white/5 rounded-2xl">
                  <div className="flex justify-between items-center text-[11px] font-bold text-white/80 pb-1.5 border-b border-white/5">
                    <span className="flex items-center gap-1.5">
                      <Activity size={13} className="text-emerald-400" />
                      {t(locale, 'controlHub.systemMonitor')}
                    </span>
                    <span className="text-[9px] font-semibold text-emerald-400 bg-emerald-500/10 px-1.5 py-0.5 rounded-md">Live</span>
                  </div>

                  <div>
                    <div className="flex justify-between text-[10px] text-white/60 mb-1">
                      <span>{t(locale, 'controlHub.cpuActivity')}</span>
                      <span className="font-mono">{cpuUsage}%</span>
                    </div>
                    <div className="w-full h-1.5 bg-white/10 rounded-full overflow-hidden">
                      <div className="h-full bg-gradient-to-r from-emerald-500 to-teal-400 rounded-full transition-all duration-[600ms] ease-out-expo" style={{ width: `${cpuUsage}%` }} />
                    </div>
                  </div>

                  <div>
                    <div className="flex justify-between text-[10px] text-white/60 mb-1">
                      <span>{t(locale, 'controlHub.memoryFootprint')}</span>
                      <span className="font-mono">{Math.round(memoryUsedBytes / 1024 / 1024)} MB / {Math.round(memoryTotalBytes / 1024 / 1024)} MB</span>
                    </div>
                    <div className="w-full h-1.5 bg-white/10 rounded-full overflow-hidden">
                      <div className="h-full bg-gradient-to-r from-emerald-500 to-teal-400 rounded-full transition-all duration-300" style={{ width: `${memoryTotalBytes > 0 ? (memoryUsedBytes / memoryTotalBytes) * 100 : 0}%` }} />
                    </div>
                  </div>

                  <div className="flex justify-between items-center pt-1 mt-1 border-t border-white/5 text-[10px]">
                    <span className="text-white/60">{t(locale, 'controlHub.sandboxMode')}</span>
                    <span className={`flex items-center gap-1 font-bold px-2 py-0.5 rounded-lg border ${
                      isSandboxActive
                        ? 'text-cyan-300 bg-cyan-500/15 border-cyan-400/20'
                        : 'text-white/40 bg-white/5 border-white/5'
                    }`}>
                      {isSandboxActive ? (
                        <><ShieldCheck size={11} />{t(locale, 'controlHub.sandboxActive')}</>
                      ) : (
                        <><ShieldAlert size={11} />{t(locale, 'controlHub.sandboxInactive')}</>
                      )}
                    </span>
                  </div>
                </div>

                {/* State & Sandbox Interactive Actions */}
                <div className="grid grid-cols-2 gap-2 mt-1">
                  <button onClick={() => { setClicks((c) => c + 1); }}
                    className="py-2.5 px-3 rounded-xl bg-white/10 hover:bg-white/15 border border-white/10 text-xs font-semibold text-center transition-all active:scale-[0.97]">
                    {t(locale, 'controlHub.incrementState').replace('{n}', clicks.toString())}
                  </button>
                  <button onClick={() => setIsSandboxActive((p) => !p)}
                    className={`py-2.5 px-3 rounded-xl border text-xs font-semibold text-center transition-all active:scale-[0.97] ${
                      isSandboxActive
                        ? 'bg-cyan-500/20 border-cyan-400/30 text-cyan-200'
                        : 'bg-white/10 hover:bg-white/15 border border-white/10 text-white'
                    }`}>
                    {t(locale, 'controlHub.toggleSandbox')}
                  </button>
                </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
