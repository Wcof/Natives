import { NextRequest, NextResponse } from 'next/server';
import { trashEntry } from '@/main/file-manager';

export async function POST(req: NextRequest) {
  try {
    const { path } = await req.json();
    if (!path) {
      return NextResponse.json({ ok: false, error: 'path is required' }, { status: 400 });
    }
    await trashEntry(path);
    return NextResponse.json({ ok: true });
  } catch (err: any) {
    console.error('[API] trashEntry error:', err);
    return NextResponse.json({ ok: false, error: err.message });
  }
}
