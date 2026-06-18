import { NextRequest, NextResponse } from 'next/server';
import { createEntry } from '@/main/file-manager';

export async function POST(req: NextRequest) {
  try {
    const { path, type } = await req.json();
    if (!path || !type) {
      return NextResponse.json({ ok: false, error: 'path and type are required' }, { status: 400 });
    }
    await createEntry(path, type);
    return NextResponse.json({ ok: true });
  } catch (err: any) {
    console.error('[API] createEntry error:', err);
    return NextResponse.json({ ok: false, error: err.message });
  }
}
