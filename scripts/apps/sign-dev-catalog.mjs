#!/usr/bin/env node
import { resolve } from 'node:path';
import { signCatalog } from './catalog-signing.mjs';
const keyPath = process.argv[2];
if (!keyPath) throw new Error('Pass an explicit local catalog signing key path');
signCatalog(resolve('extension/apps/catalog-v1.json'), resolve(keyPath));
console.log('bundled catalog signed with the matching development key');
