import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const FFMPEG = process.env.HOME + '/.workbuddy/tools/node_modules/ffmpeg-static/ffmpeg';
const IN = process.argv[2];
const W = Number(process.env.GIF_W || 960);
const FPS = Number(process.env.GIF_FPS || 12);
const OUT = IN.replace(/\.mp4$/, '.gif');
const PAL = path.join(path.dirname(IN), '.palette.png');

// 两阶段调色板：先扫全片生成专属 256 色，再用它编码
let r = spawnSync(FFMPEG, ['-y', '-i', IN, '-vf',
  `fps=${FPS},scale=${W}:-1:flags=lanczos,palettegen=stats_mode=diff`, PAL], { stdio: 'inherit' });
if (r.status !== 0) process.exit(1);

r = spawnSync(FFMPEG, ['-y', '-i', IN, '-i', PAL, '-lavfi',
  `fps=${FPS},scale=${W}:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle`,
  '-loop', '0', OUT], { stdio: 'inherit' });
if (r.status !== 0) process.exit(1);

fs.rmSync(PAL, { force: true });
console.log('gif', OUT, (fs.statSync(OUT).size / 1048576).toFixed(2), 'MB');
