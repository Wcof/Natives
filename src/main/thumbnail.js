"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/main/thumbnail.ts
var thumbnail_exports = {};
__export(thumbnail_exports, {
  generateThumb: () => generateThumb,
  setThumbCacheDir: () => setThumbCacheDir
});
module.exports = __toCommonJS(thumbnail_exports);
var fs = __toESM(require("fs"));
var path = __toESM(require("path"));
var os = __toESM(require("os"));
var crypto = __toESM(require("crypto"));

// src/lib/exec-file.ts
var import_child_process = require("child_process");
function execFilePromise(cmd, args, options) {
  return new Promise((resolve, reject) => {
    (0, import_child_process.execFile)(cmd, args, { cwd: options?.cwd, timeout: options?.timeout ?? 15e3 }, (err, stdout) => {
      if (err) reject(err);
      else resolve(stdout);
    });
  });
}

// src/main/thumbnail.ts
var IMAGE_EXTENSIONS = /* @__PURE__ */ new Set([".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tiff", ".tif", ".heic", ".avif"]);
var VIDEO_EXTENSIONS = /* @__PURE__ */ new Set([".mp4", ".mov", ".avi", ".mkv", ".webm", ".m4v"]);
var THUMBNAILABLE_EXTENSIONS = /* @__PURE__ */ new Set([...IMAGE_EXTENSIONS, ...VIDEO_EXTENSIONS, ".pdf"]);
var CACHE_DIR = path.join(os.homedir(), ".natives", "thumbs");
var MAX_CACHE_SIZE = 400 * 1024 * 1024;
var thumbInflight = /* @__PURE__ */ new Map();
function setThumbCacheDir(dir) {
  CACHE_DIR = dir;
}
function getCacheKey(filePath, width) {
  return crypto.createHash("sha256").update(`${filePath}:${width}`).digest("hex");
}
function getCachePath(cacheKey) {
  return path.join(CACHE_DIR, `${cacheKey}.jpg`);
}
function getMetaPath() {
  return path.join(CACHE_DIR, "meta.json");
}
async function readMeta() {
  try {
    const raw = await fs.promises.readFile(getMetaPath(), "utf-8");
    return JSON.parse(raw);
  } catch {
    return {};
  }
}
async function writeMeta(meta) {
  await fs.promises.mkdir(CACHE_DIR, { recursive: true });
  await fs.promises.writeFile(getMetaPath(), JSON.stringify(meta), "utf-8");
}
async function updateAccess(cacheKey) {
  const meta = await readMeta();
  if (meta[cacheKey]) {
    meta[cacheKey].lastAccess = Date.now();
    await writeMeta(meta);
  }
}
async function enforceCacheLimit() {
  const meta = await readMeta();
  const keys = Object.keys(meta);
  if (keys.length === 0) return;
  let totalSize = 0;
  for (const key of keys) {
    totalSize += meta[key].size || 0;
  }
  if (totalSize <= MAX_CACHE_SIZE) return;
  const sorted = keys.sort((a, b) => (meta[a].lastAccess || 0) - (meta[b].lastAccess || 0));
  for (const key of sorted) {
    if (totalSize <= MAX_CACHE_SIZE) break;
    const entry = meta[key];
    try {
      await fs.promises.unlink(getCachePath(key));
    } catch {
    }
    totalSize -= entry.size || 0;
    delete meta[key];
  }
  await writeMeta(meta);
}
async function generateThumb(filePath, width) {
  const thumbWidth = Math.max(48, Math.min(1600, width));
  try {
    const stat = await fs.promises.stat(filePath);
    if (!stat.isFile()) return null;
  } catch {
    return null;
  }
  const ext = path.extname(filePath).toLowerCase();
  if (!THUMBNAILABLE_EXTENSIONS.has(ext)) return null;
  const cacheKey = getCacheKey(filePath, thumbWidth);
  const cachePath = getCachePath(cacheKey);
  try {
    const buffer = await fs.promises.readFile(cachePath);
    await updateAccess(cacheKey);
    return { buffer, contentType: "image/jpeg", cached: true };
  } catch {
  }
  const inflight = thumbInflight.get(cacheKey);
  if (inflight) return inflight;
  const promise = (async () => {
    await fs.promises.mkdir(CACHE_DIR, { recursive: true });
    const tmpDir = path.join(os.tmpdir(), "natives-thumbs");
    await fs.promises.mkdir(tmpDir, { recursive: true });
    const tmpPath = path.join(tmpDir, `thumb-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.jpg`);
    try {
      if (IMAGE_EXTENSIONS.has(ext)) {
        await execFilePromise("sips", ["-Z", String(thumbWidth), "-s", "format", "jpeg", filePath, "--out", tmpPath]);
      } else {
        await execFilePromise("qlmanage", ["-t", "-s", String(thumbWidth), "-o", tmpDir, filePath]);
        const basename2 = path.basename(filePath, ext);
        const qlOutput = path.join(tmpDir, `${basename2}.png`);
        if (fs.existsSync(qlOutput)) {
          await execFilePromise("sips", ["-s", "format", "jpeg", qlOutput, "--out", tmpPath]);
          try {
            await fs.promises.unlink(qlOutput);
          } catch {
          }
        }
      }
      const buffer = await fs.promises.readFile(tmpPath);
      try {
        await fs.promises.copyFile(tmpPath, cachePath);
        const meta = await readMeta();
        meta[cacheKey] = {
          filePath,
          width: thumbWidth,
          size: buffer.length,
          lastAccess: Date.now()
        };
        await writeMeta(meta);
        await enforceCacheLimit();
      } catch {
      }
      try {
        await fs.promises.unlink(tmpPath);
      } catch {
      }
      return { buffer, contentType: "image/jpeg", cached: false };
    } catch {
      try {
        if (fs.existsSync(tmpPath)) await fs.promises.unlink(tmpPath);
      } catch {
      }
      return null;
    }
  })();
  thumbInflight.set(cacheKey, promise);
  try {
    return await promise;
  } finally {
    thumbInflight.delete(cacheKey);
  }
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  generateThumb,
  setThumbCacheDir
});
