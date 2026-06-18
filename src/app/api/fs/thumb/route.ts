import { NextRequest } from 'next/server';

export async function GET(req: NextRequest) {
  const filePath = req.nextUrl.searchParams.get('path');
  const width = parseInt(req.nextUrl.searchParams.get('w') || '320', 10);

  if (!filePath) {
    return new Response('Missing path parameter', { status: 400 });
  }

  if (filePath.includes('\0') || filePath.includes('..')) {
    return new Response('Forbidden', { status: 403 });
  }

  try {
    const { generateThumb } = await import('@/main/thumbnail');
    const result = await generateThumb(filePath, width);

    if (result) {
      return new Response(new Uint8Array(result.buffer), {
        status: 200,
        headers: {
          'Content-Type': result.contentType,
          'Content-Length': String(result.buffer.length),
          'Cache-Control': 'public, max-age=604800',
        },
      });
    } else {
      return new Response('Cannot generate thumbnail', { status: 415 });
    }
  } catch (err) {
    console.error('[API] thumb error:', err);
    return new Response('Thumbnail generation failed', { status: 500 });
  }
}
