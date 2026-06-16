import { NextRequest, NextResponse } from 'next/server';
import { renameEntry } from '@/main/file-manager';

export async function POST(req: NextRequest) {
  try {
    const { oldPath, newPath } = await req.json();
    if (!oldPath || !newPath) {
      return NextResponse.json({ ok: false, error: 'oldPath and newPath are required' }, { status: 400 });
    }
    await renameEntry(oldPath, newPath);
    return NextResponse.json({ ok: true });
  } catch (err: any) {
    console.error('[API] renameEntry error:', err);
    return NextResponse.json({ ok: false, error: err.message });
  }
}
