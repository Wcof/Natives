# Natives《本地》· 60 秒宣传片

产品宣传片，1920×1080 / 60fps，无旁白，纯字卡。用 HTML 时间轴写成，逐帧 seek 渲染成 MP4。

## 交付物

| 文件 | 规格 | 用途 |
|---|---|---|
| `natives-local.mp4` | 1920×1080 · 60fps · H.264 High · AAC 192k | 主交付，带 BGM 与音效 |
| `natives-local.gif` | 960×540 · 12fps · palette 优化 | 聊天工具 / README 预览 |
| `natives-local-poster.png` | 1920×1080 | 封面帧（t=26.0，「四个房间」） |
| `natives-local-silent.mp4` | 1920×1080 · 60fps · 无音轨 | 无声母版，方便后配旁白 |
| `natives-local.html` | 单文件，无外部依赖 | 源文件，可交互预览 |

## 预览

直接双击 `natives-local.html`。系统字体渲染，不依赖网络。

- `?t=32` 冻结在第 32 秒（渲染管线就用这个接口）
- 空格暂停 / 继续，← → 前后 1 秒
- 窗口小于 1920×1080 时自动等比缩放，导出时按 1:1 截取

## 重新生成

```bash
# 1. 逐帧渲染（5 个 Chrome worker，约 3 分钟）
FPS=60 DUR=60 node render.mjs ./natives-local.html ./natives-local-silent.mp4

# 2. 混音（BGM + 34 个 SFX cue），输出带声成片
node build-audio.mjs ./natives-local-silent.mp4 ./natives-local.mp4

# 3. 封面帧
ffmpeg -ss 26.0 -i natives-local.mp4 -frames:v 1 natives-local-poster.png

# 4. GIF
node convert.mjs ./natives-local.mp4
```

依赖：本机 Chrome（`/Applications/Google Chrome.app`）、`playwright-core`（复用仓库 `node_modules`）、静态 ffmpeg（`~/.workbuddy/tools/node_modules/ffmpeg-static/ffmpeg`）。

## 设计依据

- `导演稿.md` — 视觉系统、故事弧、10 镜分镜卡、声音时间轴
- `brand-spec.md` — 色板与字体全部取自 `extension/theme.css`（Archive / Volt 双主题），片中技术表述逐条对应仓库文档
- `direction-approved.md` — 三方向门的豁免记录

## 结构

```
0–14s   纸面。文件在这里 → AI 在那边 → 中间隔着墙
14–32s  黑场。修了一条路 → 四个房间 → 它读到了
32–60s  落定。全部在本机 → 工作台 → Natives
```

主角是一根线：它先是文件的选中行，再长成窗口，再变成墙，再变成 Native Messaging 的干线，再变成平面图的上墙，最后收成方框与圆标。全片没有一次运镜，所有尺度变化都由对象自己完成。

---

动画由 [huashu-design](https://github.com/alchaincyf/huashu-design) 的方法与工具链产出。
