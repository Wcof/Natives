import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  output: 'standalone',
  distDir: '.next',
  // Tauri: no serverExternalPackages needed — SQLite and PTY are in Rust backend
};

export default nextConfig;
