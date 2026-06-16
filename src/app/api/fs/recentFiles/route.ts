import { NextRequest, NextResponse } from 'next/server';
import { getRecentModifiedFiles } from '@/main/recent-files';

export async function GET(req: NextRequest) {
  const path = req.nextUrl.searchParams.get('path');
  if (!path) {
    return NextResponse.json({ error: 'path is required' }, { status: 400 });
  }

  try {
    const files = getRecentModifiedFiles(path);
    return NextResponse.json(files);
  } catch (err: any) {
    console.error('[API] recentFiles error:', err);
    return NextResponse.json({ error: err.message }, { status: 500 });
  }
}
