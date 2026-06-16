# Fix: /api/fs/raw Next.js API Route

## Problem

Multiple components reference `http://localhost:${httpPort}/api/fs/raw?path=...` for file previews (images, video, audio, PDF, text). In Electron mode, the custom http-server handles this route. In web mode (Next.js dev/build), no such API route exists — all previews return 404.

## Affected Components

- `FilePreview.tsx` — image/video/audio/PDF preview
- `FollowRenderer.tsx` — AI follow-mode file rendering
- `useFileContent.ts` — text content hook

## Fix

Create `src/app/api/fs/raw/route.ts` following the existing API route pattern (e.g., `listDir/route.ts`).

### Route behavior

- `GET /api/fs/raw?path=<filePath>`
- Security: reject null bytes, `..` traversal
- Read file with `fs.readFile`
- Detect MIME type from extension (reuse `getMimeType` from `file-manager.ts` or inline a simple map)
- Return file with proper `Content-Type` header
- Support `Range` headers for video/audio seeking

### Files to create/modify

| File | Action |
|------|--------|
| `src/app/api/fs/raw/route.ts` | **Create** — Next.js API route |

### Reference

- Electron implementation: `src/main/http-server.ts:239-296`
- Existing API route pattern: `src/app/api/fs/listDir/route.ts`
- MIME helper: `src/main/file-manager.ts` (`getMimeType`)
