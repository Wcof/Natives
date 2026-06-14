import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  output: 'standalone',
  distDir: '.next',
  serverExternalPackages: ['better-sqlite3', 'node-pty'],
};

export default nextConfig;
