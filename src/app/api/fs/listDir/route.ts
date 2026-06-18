import { NextRequest, NextResponse } from 'next/server';
import { listDir } from '@/main/file-manager';

export async function GET(req: NextRequest) {
  const path = req.nextUrl.searchParams.get('path');
  if (!path) {
    return NextResponse.json({ error: 'path is required' }, { status: 400 });
  }

  const sortBy = req.nextUrl.searchParams.get('sortBy') as 'name' | 'mtime' | 'size' | null;
  const sortDir = req.nextUrl.searchParams.get('sortDir') as 'asc' | 'desc' | null;
  const showHidden = req.nextUrl.searchParams.get('showHidden') === 'true';

  try {
    const entries = await listDir(path, {
      sortBy: sortBy || 'name',
      sortDir: sortDir || 'asc',
      showHidden,
    });
    return NextResponse.json(entries);
  } catch (err: any) {
    console.error('[API] listDir error:', err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
