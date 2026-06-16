import { NextRequest, NextResponse } from 'next/server';
import { getDiskUsage } from '@/main/disk-usage';

export async function GET(request: NextRequest) {
  const path = request.nextUrl.searchParams.get('path') || '/';
  try {
    const items = await getDiskUsage(path);
    return NextResponse.json(items);
  } catch (err) {
    return NextResponse.json({ error: (err as Error).message }, { status: 400 });
  }
}
