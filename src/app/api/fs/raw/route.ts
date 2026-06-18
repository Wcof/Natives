import { NextRequest } from 'next/server';
import * as fs from 'fs';
import * as path from 'path';

function getMimeType(filePath: string): string {
  const ext = path.extname(filePath).toLowerCase();
  const mimeMap: Record<string, string> = {
    '.html': 'text/html', '.htm': 'text/html',
    '.css': 'text/css', '.js': 'application/javascript', '.mjs': 'application/javascript',
    '.json': 'application/json', '.xml': 'application/xml',
    '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg',
    '.gif': 'image/gif', '.webp': 'image/webp', '.svg': 'image/svg+xml',
    '.ico': 'image/x-icon', '.bmp': 'image/bmp',
    '.mp4': 'video/mp4', '.webm': 'video/webm',
    '.pdf': 'application/pdf',
    '.zip': 'application/zip', '.tar': 'application/x-tar', '.gz': 'application/gzip',
    '.md': 'text/markdown', '.txt': 'text/plain',
    '.ts': 'text/typescript', '.tsx': 'text/typescript',
    '.py': 'text/x-python', '.rs': 'text/x-rust', '.go': 'text/x-go',
  };
  return mimeMap[ext] || 'application/octet-stream';
}

export async function GET(req: NextRequest) {
  const filePath = req.nextUrl.searchParams.get('path');

  if (!filePath) {
    return new Response('Missing path parameter', { status: 400 });
  }

  if (filePath.includes('\0') || filePath.includes('..')) {
    return new Response('Forbidden', { status: 403 });
  }

  try {
    const stat = await fs.promises.stat(filePath);
    if (!stat.isFile()) {
      return new Response('Path is not a file', { status: 400 });
    }

    const buffer = await fs.promises.readFile(filePath);
    const contentType = getMimeType(filePath);

    return new Response(new Uint8Array(buffer), {
      status: 200,
      headers: {
        'Content-Type': contentType,
        'Content-Length': String(buffer.length),
        'Cache-Control': 'no-cache',
      },
    });
  } catch (err: any) {
    if (err?.code === 'ENOENT') {
      return new Response('File not found', { status: 404 });
    }
    console.error('[API] raw error:', err);
    return new Response('Internal Server Error', { status: 500 });
  }
}
