'use client';

import { useState, useEffect, useRef } from 'react';
import dynamic from 'next/dynamic';
import { t, useLocale } from '@/i18n';
import { useTheme } from '@/context/ThemeContext';
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
  Cpu,
  Sliders,
  Check,
  RotateCcw,
  Languages,
  ImageIcon,
  Layers,
  Laptop,
} from 'lucide-react';
import { SPACING, BORDER_RADIUS, TRANSITION, FONT_SIZE } from '@/lib/design-tokens';

const LiquidGlass = dynamic(() => import('liquid-glass-react'), { ssr: false });

// Background mockups to showcase refraction distortion
const WALLPAPERS = [
  'https://images.unsplash.com/photo-1579033461380-adb47c3eb938?q=80&w=800&auto=format&fit=crop', // Cosmic Aurora lights
];

export default function ControlHubWidget() {
  const locale = useLocale();
  const { themeId, setTheme } = useTheme();
  const containerRef = useRef<HTMLDivElement>(null);

  // --- Widget State ---
  const [activeTab, setActiveTab] = useState<'control' | 'settings'>('control');
  const [clicks, setClicks] = useState(0);
  const [cpuUsage, setCpuUsage] = useState(38);
  const [memoryUsage, setMemoryUsage] = useState(132);
  const [isSandboxActive, setIsSandboxActive] = useState(false);
  const [isWifiOn, setIsWifiOn] = useState(true);
  const [isBluetoothOn, setIsBluetoothOn] = useState(true);
  const [isAirdropOn, setIsAirdropOn] = useState(false);
  const [isDndOn, setIsDndOn] = useState(false);
  const [brightness, setBrightness] = useState(75);
  const [volume, setVolume] = useState(50);

  // --- Glass Effects Config (Shared between settings & preview) ---
  const [displacementScale, setDisplacementScale] = useState(64);
  const [blurAmount, setBlurAmount] = useState(0.4);
  const [saturation, setSaturation] = useState(135);
  const [aberrationIntensity, setAberrationIntensity] = useState(2);
  const [elasticity, setElasticity] = useState(0);
  const [cornerRadius, setCornerRadius] = useState(28);
  const [overLight, setOverLight] = useState(false);
  const [mode, setMode] = useState<'standard' | 'polar' | 'prominent' | 'shader'>('standard');

  // --- Wallpaper & Blobs Mockup State ---
  const [showWallpaper, setShowWallpaper] = useState(true);
  const [wallpaperIndex, setWallpaperIndex] = useState(0);
  const [showBlobs, setShowBlobs] = useState(true);
  const [hasLoaded, setHasLoaded] = useState(false);

  // --- Config Database Persistence ---
  const CONFIG_DB_KEY = 'settings:controlHubVisuals';

  // Load configuration from database on mount
  useEffect(() => {
    async function loadSavedConfig() {
      const api = window.nativesAPI;
      if (!api?.db?.get) return;
      try {
        const savedStr = await api.db.get(CONFIG_DB_KEY);
        if (savedStr) {
          const saved = JSON.parse(savedStr as string);
          if (saved) {
            if (typeof saved.displacementScale === 'number') setDisplacementScale(saved.displacementScale);
            if (typeof saved.blurAmount === 'number') setBlurAmount(saved.blurAmount);
            if (typeof saved.saturation === 'number') setSaturation(saved.saturation);
            if (typeof saved.aberrationIntensity === 'number') setAberrationIntensity(saved.aberrationIntensity);
            if (typeof saved.elasticity === 'number') setElasticity(saved.elasticity);
            if (typeof saved.cornerRadius === 'number') setCornerRadius(saved.cornerRadius);
            if (typeof saved.overLight === 'boolean') setOverLight(saved.overLight);
            if (['standard', 'polar', 'prominent', 'shader'].includes(saved.mode)) setMode(saved.mode);
            if (typeof saved.showWallpaper === 'boolean') setShowWallpaper(saved.showWallpaper);
            if (typeof saved.showBlobs === 'boolean') setShowBlobs(saved.showBlobs);
          }
        }
      } catch (err) {
        console.warn('Failed to load saved visual config:', err);
      }
    }
    loadSavedConfig()
      .finally(() => setHasLoaded(true));

    // Re-load config when another tab updates it in the database
    const api = window.nativesAPI;
    const unsubscribe = api?.onDbStateChanged?.((_event: unknown, channel: string, data: any) => {
      if (channel === 'module_data' && data?.key === CONFIG_DB_KEY) {
        loadSavedConfig();
      }
    });
    return () => {
      if (unsubscribe) unsubscribe();
    };
  }, []);

  // Save configuration to database with a 500ms debounce
  useEffect(() => {
    if (!hasLoaded) return;
    const api = window.nativesAPI;
    if (!api?.db?.set) return;
    const timer = setTimeout(() => {
      const config = {
        displacementScale,
        blurAmount,
        saturation,
        aberrationIntensity,
        elasticity,
        cornerRadius,
        overLight,
        mode,
        showWallpaper,
        showBlobs,
      };
      api.db.set(CONFIG_DB_KEY, JSON.stringify(config)).catch((err) => {
        console.warn('Failed to save visual config:', err);
      });
    }, 500);
    return () => clearTimeout(timer);
  }, [
    hasLoaded, displacementScale, blurAmount, saturation, aberrationIntensity,
    elasticity, cornerRadius, overLight, mode, showWallpaper, showBlobs,
  ]);

  // --- CPU Simulation ---
  useEffect(() => {
    const timer = setInterval(() => {
      setCpuUsage((prev) => {
        const delta = Math.floor(Math.random() * 11) - 5;
        return Math.max(12, Math.min(94, prev + delta));
      });
    }, 1500);
    return () => clearInterval(timer);
  }, []);

  // --- Language Toggle helper ---
  const toggleLanguage = async () => {
    const nextLocale = locale.startsWith('zh') ? 'en' : 'zh';
    try {
      if (window.nativesAPI?.setLocale) await window.nativesAPI.setLocale(nextLocale);
    } catch { /* ignore */ }
    window.dispatchEvent(new CustomEvent('locale-changed', { detail: nextLocale }));
  };

  // --- Reset Config ---
  const resetToDefaults = () => {
    setDisplacementScale(64);
    setBlurAmount(0.4);
    setSaturation(135);
    setAberrationIntensity(2);
    setElasticity(0);
    setCornerRadius(28);
    setOverLight(false);
    setMode('standard');
    setTheme('terminal-volt');
    setShowWallpaper(false);
    setShowBlobs(false);
  };

  // Window draggability helper styles
  const dragStyle: React.CSSProperties = { WebkitAppRegion: 'drag' } as any;
  const noDragStyle: React.CSSProperties = { WebkitAppRegion: 'no-drag' } as any;

  return (
    <div
      ref={containerRef}
      className="liquid-glass-container select-none"
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
      }}
    >
      {/* Background Desktop Wallpaper Mockup */}
      {showWallpaper && (
        <div
          style={{
            position: 'absolute', inset: 0,
            backgroundImage: `url(${WALLPAPERS[wallpaperIndex]}), radial-gradient(at 10% 20%, rgba(0, 255, 156, 0.18) 0px, transparent 50%), radial-gradient(at 90% 10%, rgba(255, 121, 63, 0.22) 0px, transparent 50%), radial-gradient(at 50% 80%, rgba(0, 221, 255, 0.15) 0px, transparent 50%), linear-gradient(135deg, #0d0f12 0%, #1c2027 100%)`,
            backgroundSize: 'cover', backgroundPosition: 'center',
            zIndex: 0, transition: 'background-image 0.5s ease-in-out',
          }}
        />
      )}

      {/* Background Animated Blobs */}
      {showBlobs && (
        <div className="liquid-blob-wrapper" style={{ zIndex: 1 }}>
          <div className="liquid-blob liquid-blob-1" />
          <div className="liquid-blob liquid-blob-2" />
          <div className="liquid-blob liquid-blob-3" />
        </div>
      )}

      {/* Main Glass Widget Card */}
      <LiquidGlass
        displacementScale={displacementScale}
        blurAmount={blurAmount}
        saturation={saturation}
        aberrationIntensity={aberrationIntensity}
        elasticity={elasticity}
        cornerRadius={cornerRadius}
        padding="0px"
        mode={mode}
        mouseContainer={containerRef}
        overLight={overLight}
        style={{
          width: 390,
          position: 'relative',
          zIndex: 10,
        }}
      >
        <div
          className="flex flex-col text-white"
          style={{
            width: '390px',
            minHeight: '490px',
            padding: '24px',
            boxSizing: 'border-box',
            ...dragStyle,
          }}
        >
          {/* Header */}
          <div className="flex flex-col items-center justify-center text-center mb-5 mt-1 pb-1 border-b border-white/10">
            <h1
              className="font-bold tracking-tight text-shadow-md"
              style={{
                fontSize: FONT_SIZE.heading + 2,
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

          {/* Segmented Tab Bar */}
          <div
            className="flex p-1 bg-black/20 rounded-xl mb-5 border border-white/5 backdrop-blur-md"
            style={noDragStyle}
          >
            <button
              onClick={() => setActiveTab('control')}
              className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 text-xs font-semibold rounded-lg transition-all duration-200 ${
                activeTab === 'control'
                  ? 'bg-white/15 text-white shadow-sm'
                  : 'text-white/60 hover:text-white hover:bg-white/5'
              }`}
            >
              <Cpu size={13} />
              {t(locale, 'controlHub.tabControls')}
            </button>
            <button
              onClick={() => setActiveTab('settings')}
              className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 text-xs font-semibold rounded-lg transition-all duration-200 ${
                activeTab === 'settings'
                  ? 'bg-white/15 text-white shadow-sm'
                  : 'text-white/60 hover:text-white hover:bg-white/5'
              }`}
            >
              <Sliders size={13} />
              {t(locale, 'controlHub.tabSettings')}
            </button>
          </div>

          {/* Tab Contents */}
          <div className="flex-1 flex flex-col justify-between" style={noDragStyle}>
            {activeTab === 'control' ? (
              // --- CONTROL HUB VIEW ---
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
                      <span className="font-mono">{memoryUsage} MB / 256 MB</span>
                    </div>
                    <div className="w-full h-1.5 bg-white/10 rounded-full overflow-hidden">
                      <div className="h-full bg-gradient-to-r from-emerald-500 to-teal-400 rounded-full transition-all duration-300" style={{ width: `${(memoryUsage / 256) * 100}%` }} />
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
                  <button onClick={() => { setClicks((c) => c + 1); setMemoryUsage((m) => Math.min(256, m + 6)); }}
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
            ) : (
              // --- VISUAL SETTINGS VIEW ---
              <div className="flex flex-col gap-4 animate-fade-in overflow-y-auto max-h-[385px] pr-1.5 scrollbar-thin">
                {/* Refraction Mode Radio Selectors */}
                <div className="flex flex-col gap-1.5">
                  <span className="text-[10px] font-bold text-white/60 uppercase tracking-wider">
                    {t(locale, 'controlHub.settings.refractionMode')}
                  </span>
                  <div className="grid grid-cols-2 gap-1.5">
                    {(['standard', 'polar', 'prominent', 'shader'] as const).map((m) => (
                      <button key={m} onClick={() => setMode(m)}
                        className={`py-1.5 px-2.5 text-[10px] font-semibold rounded-lg border text-left flex items-center justify-between transition-all ${
                          mode === m ? 'bg-blue-600/40 border-blue-400/30 text-white' : 'bg-white/5 border-white/5 text-white/60 hover:bg-white/10'
                        }`}>
                        <span className="capitalize">
                          {m === 'standard' && t(locale, 'controlHub.settings.modeStandard')}
                          {m === 'polar' && t(locale, 'controlHub.settings.modePolar')}
                          {m === 'prominent' && t(locale, 'controlHub.settings.modeProminent')}
                          {m === 'shader' && t(locale, 'controlHub.settings.modeShader')}
                        </span>
                        {mode === m && <Check size={11} className="text-blue-300" />}
                      </button>
                    ))}
                  </div>
                </div>

                {/* Effect Range Sliders */}
                <div className="flex flex-col gap-3.5 p-4 bg-white/5 border border-white/5 rounded-2xl">
                  <div className="flex flex-col gap-1.5">
                    <div className="flex justify-between items-center text-[10px] text-white/60">
                      <span>{t(locale, 'controlHub.settings.displacementScale')}</span>
                      <span className="font-mono text-blue-300 font-bold">{displacementScale}</span>
                    </div>
                    <input type="range" min="0" max="150" value={displacementScale}
                      onChange={(e) => setDisplacementScale(Number(e.target.value))}
                      className="w-full h-1 bg-white/10 rounded-lg appearance-none cursor-pointer accent-blue-500" />
                  </div>

                  <div className="flex flex-col gap-1.5 border-t border-white/5 pt-3">
                    <div className="flex justify-between items-center text-[10px] text-white/60">
                      <span>{t(locale, 'controlHub.settings.blurAmount')}</span>
                      <span className="font-mono text-emerald-300 font-bold">{blurAmount.toFixed(2)}</span>
                    </div>
                    <input type="range" min="0" max="1" step="0.05" value={blurAmount}
                      onChange={(e) => setBlurAmount(Number(e.target.value))}
                      className="w-full h-1 bg-white/10 rounded-lg appearance-none cursor-pointer accent-emerald-400" />
                  </div>

                  <div className="flex flex-col gap-1.5 border-t border-white/5 pt-3">
                    <div className="flex justify-between items-center text-[10px] text-white/60">
                      <span>{t(locale, 'controlHub.settings.saturation')}</span>
                      <span className="font-mono text-purple-300 font-bold">{saturation}%</span>
                    </div>
                    <input type="range" min="100" max="250" step="5" value={saturation}
                      onChange={(e) => setSaturation(Number(e.target.value))}
                      className="w-full h-1 bg-white/10 rounded-lg appearance-none cursor-pointer accent-purple-400" />
                  </div>

                  <div className="flex flex-col gap-1.5 border-t border-white/5 pt-3">
                    <div className="flex justify-between items-center text-[10px] text-white/60">
                      <span>{t(locale, 'controlHub.settings.aberration')}</span>
                      <span className="font-mono text-cyan-300 font-bold">{aberrationIntensity}</span>
                    </div>
                    <input type="range" min="0" max="10" step="1" value={aberrationIntensity}
                      onChange={(e) => setAberrationIntensity(Number(e.target.value))}
                      className="w-full h-1 bg-white/10 rounded-lg appearance-none cursor-pointer accent-cyan-400" />
                  </div>

                  <div className="flex flex-col gap-1.5 border-t border-white/5 pt-3">
                    <div className="flex justify-between items-center text-[10px] text-white/60">
                      <span>{t(locale, 'controlHub.settings.elasticity')}</span>
                      <span className="font-mono text-orange-300 font-bold">{elasticity.toFixed(2)}</span>
                    </div>
                    <input type="range" min="0" max="0.8" step="0.05" value={elasticity}
                      onChange={(e) => setElasticity(Number(e.target.value))}
                      className="w-full h-1 bg-white/10 rounded-lg appearance-none cursor-pointer accent-orange-400" />
                  </div>

                  <div className="flex flex-col gap-1.5 border-t border-white/5 pt-3">
                    <div className="flex justify-between items-center text-[10px] text-white/60">
                      <span>{t(locale, 'controlHub.settings.radius')}</span>
                      <span className="font-mono text-pink-300 font-bold">{cornerRadius}px</span>
                    </div>
                    <input type="range" min="12" max="48" step="2" value={cornerRadius}
                      onChange={(e) => setCornerRadius(Number(e.target.value))}
                      className="w-full h-1 bg-white/10 rounded-lg appearance-none cursor-pointer accent-pink-400" />
                  </div>
                </div>

                {/* Extra Toggles & Settings */}
                <div className="flex flex-col gap-3 p-4 bg-white/5 border border-white/5 rounded-2xl">
                  <div className="flex items-center justify-between">
                    <span className="text-[10px] font-bold text-white/80 flex items-center gap-1">
                      <ImageIcon size={11} className="text-blue-300" />
                      {t(locale, 'controlHub.settings.wallpaperMockup')}
                    </span>
                    <div className="flex items-center gap-2">
                      {showWallpaper && (
                        <button onClick={() => setWallpaperIndex((prev) => (prev + 1) % WALLPAPERS.length)}
                          className="px-2 py-0.5 bg-white/10 hover:bg-white/15 text-[9px] font-semibold border border-white/10 rounded-md transition-all active:scale-[0.95]">
                          Cycle ({wallpaperIndex + 1}/{WALLPAPERS.length})
                        </button>
                      )}
                      <input type="checkbox" checked={showWallpaper}
                        onChange={(e) => setShowWallpaper(e.target.checked)}
                        className="w-4 h-4 rounded accent-blue-500 cursor-pointer bg-black/30 border-white/10" />
                    </div>
                  </div>

                  <div className="flex items-center justify-between border-t border-white/5 pt-3">
                    <span className="text-[10px] font-bold text-white/80 flex items-center gap-1">
                      <Layers size={11} className="text-blue-300" />
                      {t(locale, 'controlHub.settings.blobsMockup')}
                    </span>
                    <input type="checkbox" checked={showBlobs}
                      onChange={(e) => setShowBlobs(e.target.checked)}
                      className="w-4 h-4 rounded accent-blue-500 cursor-pointer bg-black/30 border-white/10" />
                  </div>

                  <div className="flex items-center justify-between border-t border-white/5 pt-3">
                    <div className="flex flex-col text-left w-[80%]">
                      <span className="text-[10px] font-bold text-white/80 flex items-center gap-1">
                        <Layers size={11} className="text-blue-300" />
                        {t(locale, 'controlHub.settings.overLight')}
                      </span>
                      <span className="text-[9px] text-white/40 leading-snug">{t(locale, 'controlHub.settings.overLightDesc')}</span>
                    </div>
                    <input type="checkbox" checked={overLight}
                      onChange={(e) => setOverLight(e.target.checked)}
                      className="w-4 h-4 rounded accent-blue-500 cursor-pointer bg-black/30 border-white/10 shrink-0" />
                  </div>

                  <div className="flex items-center justify-between border-t border-white/5 pt-3">
                    <span className="text-[10px] font-bold text-white/80 flex items-center gap-1">
                      <Laptop size={11} className="text-blue-300" />
                      {t(locale, 'controlHub.settings.themeSelection')}
                    </span>
                    <div className="flex gap-1 bg-black/35 p-0.5 rounded-lg border border-white/5">
                      <button onClick={() => setTheme('terminal-volt')}
                        className={`px-2 py-1 text-[9px] font-bold rounded-md transition-all ${
                          themeId === 'terminal-volt' ? 'bg-blue-600/70 text-white shadow-sm' : 'text-white/50 hover:text-white'
                        }`}>Dark</button>
                      <button onClick={() => setTheme('frosted-jasmine')}
                        className={`px-2 py-1 text-[9px] font-bold rounded-md transition-all ${
                          themeId === 'frosted-jasmine' ? 'bg-blue-600/70 text-white shadow-sm' : 'text-white/50 hover:text-white'
                        }`}>Light</button>
                    </div>
                  </div>
                </div>

                {/* Reset defaults & Language controls */}
                <div className="grid grid-cols-2 gap-2 mt-1">
                  <button onClick={resetToDefaults}
                    className="flex items-center justify-center gap-1 py-2 px-3 rounded-xl bg-white/10 hover:bg-white/15 border border-white/10 text-[10px] font-bold text-center transition-all active:scale-[0.97]">
                    <RotateCcw size={11} />
                    {t(locale, 'controlHub.settings.reset')}
                  </button>
                  <button onClick={toggleLanguage}
                    className="flex items-center justify-center gap-1 py-2 px-3 rounded-xl bg-white/10 hover:bg-white/15 border border-white/10 text-[10px] font-bold text-center transition-all active:scale-[0.97]">
                    <Languages size={11} />
                    {locale.startsWith('zh') ? 'English' : '简体中文'}
                  </button>
                </div>

                <p className="text-[9px] text-yellow-300/80 leading-normal bg-yellow-500/10 p-2.5 rounded-xl border border-yellow-500/10 mt-1">
                  {t(locale, 'controlHub.settings.firefoxWarning')}
                </p>
              </div>
            )}
          </div>
        </div>
      </LiquidGlass>
    </div>
  );
}
