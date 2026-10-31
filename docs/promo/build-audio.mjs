import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const FFMPEG = process.env.HOME + '/.workbuddy/tools/node_modules/ffmpeg-static/ffmpeg';
const SFX = process.env.HOME + '/.workbuddy/skills/huashu-design/assets/sfx/';
const BGM = process.env.HOME + '/.workbuddy/skills/huashu-design/assets/bgm-tech.mp3';
const VIDEO = process.argv[2];
const OUT = process.argv[3] || VIDEO.replace(/\.mp4$/, '.mp4');

// 时间轴：与动画里的动作逐帧对齐
const CUES = [
  [1.22, 'ui/click-soft', 0.50],
  [1.80, 'ui/focus', 0.40],
  [4.18, 'transition/slide-in', 0.55],
  [5.52, 'ui/click-soft', 0.40],
  [5.86, 'keyboard/type', 0.50],
  [7.70, 'impact/drop-thud', 0.50],
  [9.34, 'transition/whoosh-fast', 0.45],
  [13.48, 'transition/whoosh', 0.50],
  [14.86, 'progress/loading-tick', 0.50],
  [15.36, 'progress/loading-tick', 0.45],
  [15.86, 'progress/loading-tick', 0.45],
  [16.55, 'transition/swipe-horizontal', 0.35],
  [17.88, 'magic/sparkle', 0.55],
  [20.34, 'transition/dissolve', 0.40],
  [20.62, 'ui/click-soft', 0.45],
  [22.52, 'container/card-snap', 0.40],
  [27.18, 'transition/whoosh-fast', 0.50],
  [27.78, 'ui/click-soft', 0.40],
  [28.32, 'terminal/output-appear', 0.40],
  [28.58, 'terminal/output-appear', 0.40],
  [28.84, 'terminal/output-appear', 0.40],
  [29.55, 'progress/loading-tick', 0.40],
  [30.75, 'progress/complete-done', 0.50],
  [31.92, 'impact/drop-thud', 0.80],
  [32.00, 'magic/transform', 0.50],
  [32.72, 'ui/click-soft', 0.40],
  [33.28, 'ui/click-soft', 0.40],
  [37.06, 'ui/focus', 0.45],
  [38.90, 'container/stack-collapse', 0.55],
  [44.04, 'ui/click', 0.40],
  [45.20, 'transition/whoosh', 0.50],
  [46.70, 'magic/sparkle', 0.45],
  [48.22, 'impact/brand-stamp', 0.85],
  [54.62, 'ui/click-soft', 0.35]
];

const missing = CUES.map(c => SFX + c[1] + '.mp3').filter(f => !fs.existsSync(f));
if (missing.length) { console.error('missing sfx', missing); process.exit(1); }

const args = ['-y', '-i', BGM];
for (const c of CUES) args.push('-i', SFX + c[1] + '.mp3');

const parts = [];
// BGM：1s 淡入 + 46s 后缓慢让位 + 结尾 3.6s 淡出
parts.push(`[0:a]atrim=0:60,asetpts=PTS-STARTPTS,volume=0.86,` +
  `volume='if(gt(t,46),max(0.62,1-(t-46)/14*0.38),1)':eval=frame,` +
  `afade=t=in:st=0:d=1.0,afade=t=out:st=56.4:d=3.6[bg]`);

CUES.forEach((c, i) => {
  const [t, , g] = c;
  parts.push(`[${i + 1}:a]volume=${g},adelay=${Math.round(t * 1000)}:all=1[s${i}]`);
});

const mixIn = ['[bg]', ...CUES.map((_, i) => `[s${i}]`)].join('');
parts.push(`${mixIn}amix=inputs=${CUES.length + 1}:normalize=0:dropout_transition=0,` +
  `alimiter=limit=0.95:level=false,aresample=48000[a]`);

const AAC = path.join(path.dirname(VIDEO), '.audio.m4a');
let r = spawnSync(FFMPEG, [...args, '-filter_complex', parts.join(';'), '-map', '[a]',
  '-c:a', 'aac', '-b:a', '192k', '-t', '60', AAC], { stdio: 'inherit' });
if (r.status !== 0) process.exit(1);

r = spawnSync(FFMPEG, ['-y', '-i', VIDEO, '-i', AAC, '-map', '0:v:0', '-map', '1:a:0',
  '-c:v', 'copy', '-c:a', 'aac', '-b:a', '192k', '-shortest', '-movflags', '+faststart', OUT],
  { stdio: 'inherit' });
if (r.status !== 0) process.exit(1);

fs.rmSync(AAC, { force: true });
const s = fs.statSync(OUT);
console.log('muxed', OUT, (s.size / 1048576).toFixed(2), 'MB');
