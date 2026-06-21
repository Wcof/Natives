import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  output: 'export',
  distDir: 'out',
  // Fix "multiple lockfiles" warning: set root to the project directory
  outputFileTracingRoot: process.cwd(),
  // Tauri: static export, no server needed
  trailingSlash: true,
};

export default nextConfig;
