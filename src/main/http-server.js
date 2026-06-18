"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __esm = (fn, res, err) => function __init() {
  if (err) throw err[0];
  try {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  } catch (e) {
    throw err = [e], e;
  }
};
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

// src/lib/exec-file.ts
function execFilePromise(cmd, args, options) {
  return new Promise((resolve3, reject) => {
    (0, import_child_process.execFile)(cmd, args, { cwd: options?.cwd, timeout: options?.timeout ?? 15e3 }, (err, stdout) => {
      if (err) reject(err);
      else resolve3(stdout);
    });
  });
}
var import_child_process;
var init_exec_file = __esm({
  "src/lib/exec-file.ts"() {
    "use strict";
    import_child_process = require("child_process");
  }
});

// src/main/thumbnail.ts
var thumbnail_exports = {};
__export(thumbnail_exports, {
  generateThumb: () => generateThumb,
  setThumbCacheDir: () => setThumbCacheDir
});
function setThumbCacheDir(dir) {
  CACHE_DIR = dir;
}
function getCacheKey(filePath, width) {
  return crypto.createHash("sha256").update(`${filePath}:${width}`).digest("hex");
}
function getCachePath(cacheKey) {
  return path2.join(CACHE_DIR, `${cacheKey}.jpg`);
}
function getMetaPath() {
  return path2.join(CACHE_DIR, "meta.json");
}
async function readMeta() {
  try {
    const raw = await fs2.promises.readFile(getMetaPath(), "utf-8");
    return JSON.parse(raw);
  } catch {
    return {};
  }
}
async function writeMeta(meta) {
  await fs2.promises.mkdir(CACHE_DIR, { recursive: true });
  await fs2.promises.writeFile(getMetaPath(), JSON.stringify(meta), "utf-8");
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
      await fs2.promises.unlink(getCachePath(key));
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
    const stat = await fs2.promises.stat(filePath);
    if (!stat.isFile()) return null;
  } catch {
    return null;
  }
  const ext = path2.extname(filePath).toLowerCase();
  if (!THUMBNAILABLE_EXTENSIONS.has(ext)) return null;
  const cacheKey = getCacheKey(filePath, thumbWidth);
  const cachePath = getCachePath(cacheKey);
  try {
    const buffer = await fs2.promises.readFile(cachePath);
    await updateAccess(cacheKey);
    return { buffer, contentType: "image/jpeg", cached: true };
  } catch {
  }
  const inflight = thumbInflight.get(cacheKey);
  if (inflight) return inflight;
  const promise = (async () => {
    await fs2.promises.mkdir(CACHE_DIR, { recursive: true });
    const tmpDir = path2.join(os2.tmpdir(), "natives-thumbs");
    await fs2.promises.mkdir(tmpDir, { recursive: true });
    const tmpPath = path2.join(tmpDir, `thumb-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.jpg`);
    try {
      if (IMAGE_EXTENSIONS.has(ext)) {
        await execFilePromise("sips", ["-Z", String(thumbWidth), "-s", "format", "jpeg", filePath, "--out", tmpPath]);
      } else {
        await execFilePromise("qlmanage", ["-t", "-s", String(thumbWidth), "-o", tmpDir, filePath]);
        const basename4 = path2.basename(filePath, ext);
        const qlOutput = path2.join(tmpDir, `${basename4}.png`);
        if (fs2.existsSync(qlOutput)) {
          await execFilePromise("sips", ["-s", "format", "jpeg", qlOutput, "--out", tmpPath]);
          try {
            await fs2.promises.unlink(qlOutput);
          } catch {
          }
        }
      }
      const buffer = await fs2.promises.readFile(tmpPath);
      try {
        await fs2.promises.copyFile(tmpPath, cachePath);
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
        await fs2.promises.unlink(tmpPath);
      } catch {
      }
      return { buffer, contentType: "image/jpeg", cached: false };
    } catch {
      try {
        if (fs2.existsSync(tmpPath)) await fs2.promises.unlink(tmpPath);
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
var fs2, path2, os2, crypto, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS, THUMBNAILABLE_EXTENSIONS, CACHE_DIR, MAX_CACHE_SIZE, thumbInflight;
var init_thumbnail = __esm({
  "src/main/thumbnail.ts"() {
    "use strict";
    fs2 = __toESM(require("fs"));
    path2 = __toESM(require("path"));
    os2 = __toESM(require("os"));
    crypto = __toESM(require("crypto"));
    init_exec_file();
    IMAGE_EXTENSIONS = /* @__PURE__ */ new Set([".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tiff", ".tif", ".heic", ".avif"]);
    VIDEO_EXTENSIONS = /* @__PURE__ */ new Set([".mp4", ".mov", ".avi", ".mkv", ".webm", ".m4v"]);
    THUMBNAILABLE_EXTENSIONS = /* @__PURE__ */ new Set([...IMAGE_EXTENSIONS, ...VIDEO_EXTENSIONS, ".pdf"]);
    CACHE_DIR = path2.join(os2.homedir(), ".natives", "thumbs");
    MAX_CACHE_SIZE = 400 * 1024 * 1024;
    thumbInflight = /* @__PURE__ */ new Map();
  }
});

// src/main/http-server.ts
var http_server_exports = {};
__export(http_server_exports, {
  getPort: () => getPort,
  setBridgeHandler: () => setBridgeHandler,
  setTokenVerifier: () => setTokenVerifier,
  startServer: () => startServer,
  stopServer: () => stopServer,
  validateHost: () => validateHost,
  validateOrigin: () => validateOrigin
});
module.exports = __toCommonJS(http_server_exports);
var http = __toESM(require("http"));
var fs3 = __toESM(require("fs"));
var path3 = __toESM(require("path"));
var crypto2 = __toESM(require("crypto"));
var os3 = __toESM(require("os"));
var import_child_process2 = require("child_process");

// src/main/file-manager.ts
var fs = __toESM(require("fs"));
var path = __toESM(require("path"));
var os = __toESM(require("os"));
function expandTilde(p) {
  if (p === "~" || p.startsWith("~/") || p.startsWith("~\\")) {
    return path.join(os.homedir(), p.slice(1));
  }
  return p;
}
var MAX_FULL_READ = 2 * 1024 * 1024;
var MAX_TRUNCATED_READ = 256 * 1024;
var MIME_TYPES = {
  ".txt": "text/plain",
  ".md": "text/markdown",
  ".html": "text/html",
  ".htm": "text/html",
  ".css": "text/css",
  ".js": "application/javascript",
  ".mjs": "application/javascript",
  ".cjs": "application/javascript",
  ".json": "application/json",
  ".xml": "application/xml",
  ".yaml": "text/yaml",
  ".yml": "text/yaml",
  ".toml": "text/toml",
  ".ts": "application/typescript",
  ".tsx": "application/typescript",
  ".jsx": "application/javascript",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".ico": "image/x-icon",
  ".bmp": "image/bmp",
  ".pdf": "application/pdf",
  ".zip": "application/zip",
  ".tar": "application/x-tar",
  ".gz": "application/gzip",
  ".mp3": "audio/mpeg",
  ".wav": "audio/wav",
  ".ogg": "audio/ogg",
  ".mp4": "video/mp4",
  ".webm": "video/webm",
  ".mov": "video/quicktime"
};
function getMimeType(filePath) {
  const lower = filePath.toLowerCase();
  for (const [ext, mime] of Object.entries(MIME_TYPES)) {
    if (lower.endsWith(ext)) return mime;
  }
  return "application/octet-stream";
}
async function streamFile(filePath, range) {
  filePath = expandTilde(filePath);
  const stat = await fs.promises.stat(filePath);
  if (!stat.isFile()) {
    throw Object.assign(new Error(`Not a file: ${filePath}`), { code: "EISDIR" });
  }
  const totalSize = stat.size;
  const contentType = getMimeType(filePath);
  if (range) {
    const start = Math.max(0, range.start);
    const end = range.end !== void 0 ? Math.min(range.end, totalSize - 1) : totalSize - 1;
    const chunkSize = end - start + 1;
    const stream = fs.createReadStream(filePath, { start, end });
    const contentRange = `bytes ${start}-${end}/${totalSize}`;
    return { stream, totalSize, contentRange, contentType };
  }
  return {
    stream: fs.createReadStream(filePath),
    totalSize,
    contentType
  };
}
var ALLOWED_ROOTS = [
  os.homedir(),
  "/tmp",
  "/private/tmp"
];

// src/main/http-server.ts
var HEIC_EXT = /* @__PURE__ */ new Set([".heic", ".heif"]);
var THUMB_DIR = path3.join(os3.homedir(), ".natives", "thumbs");
var thumbInflight2 = /* @__PURE__ */ new Map();
function heicCacheKey(file, mtimeMs) {
  return crypto2.createHash("md5").update(`${file}:${mtimeMs}`).digest("hex");
}
async function pruneThumbs() {
  try {
    const files = await fs3.promises.readdir(THUMB_DIR);
    let total = 0;
    const entries = [];
    for (const f of files) {
      if (f === "meta.json") continue;
      try {
        const s = await fs3.promises.stat(path3.join(THUMB_DIR, f));
        total += s.size;
        entries.push({ name: f, size: s.size, atime: s.atimeMs });
      } catch {
      }
    }
    if (total <= 400 * 1024 * 1024) return;
    entries.sort((a, b) => a.atime - b.atime);
    for (const e of entries) {
      if (total <= 380 * 1024 * 1024) break;
      try {
        await fs3.promises.unlink(path3.join(THUMB_DIR, e.name));
      } catch {
      }
      total -= e.size;
    }
  } catch {
  }
}
async function serveHeicAsJpegPath(file, stat) {
  const key = heicCacheKey(file, stat.mtimeMs);
  const cacheFile = path3.join(THUMB_DIR, `${key}.heic.jpg`);
  try {
    await fs3.promises.access(cacheFile);
    return cacheFile;
  } catch {
  }
  const inflight = thumbInflight2.get(key);
  if (inflight) return inflight;
  const promise = (async () => {
    await fs3.promises.mkdir(THUMB_DIR, { recursive: true });
    try {
      await new Promise((resolve3, reject) => {
        const proc = (0, import_child_process2.execFile)("sips", ["-s", "format", "jpeg", file, "--out", cacheFile], { timeout: 15e3 });
        proc.on("error", reject);
        proc.on("close", (code) => code === 0 ? resolve3() : reject(new Error(`sips exit ${code}`)));
      });
      await pruneThumbs();
      return cacheFile;
    } catch {
      return null;
    }
  })();
  thumbInflight2.set(key, promise);
  try {
    return await promise;
  } finally {
    thumbInflight2.delete(key);
  }
}
function isHeic(filePath) {
  return HEIC_EXT.has(path3.extname(filePath).toLowerCase());
}
function getModulesDir() {
  if (process.env.NATIVES_DB_DIR) {
    return path3.join(process.env.NATIVES_DB_DIR, "modules");
  }
  return path3.join(process.env.HOME || "~", ".natives", "modules");
}
var SDK_PATH = path3.join(__dirname, "..", "lib", "bridge-sdk.js");
var server = null;
var activePort = 0;
var verifyToken = null;
function setTokenVerifier(verifier) {
  verifyToken = verifier;
}
var bridgeHandler = null;
function setBridgeHandler(handler) {
  bridgeHandler = handler;
}
function extractHostname(host) {
  if (host.startsWith("[")) {
    const end = host.indexOf("]");
    return end !== -1 ? host.slice(1, end) : host;
  }
  return host.split(":")[0];
}
function validateHost(host) {
  if (!host) return false;
  const hostname = extractHostname(host);
  const allowed = ["localhost", "127.0.0.1", "::1"];
  return allowed.includes(hostname);
}
function validateOrigin(method, origin, referer) {
  if (method === "GET") return true;
  if (origin) {
    try {
      const url = new URL(origin);
      return ["localhost", "127.0.0.1", "::1", "[::1]"].includes(url.hostname);
    } catch {
      return false;
    }
  }
  if (referer) {
    try {
      const url = new URL(referer);
      return ["localhost", "127.0.0.1", "::1", "[::1]"].includes(url.hostname);
    } catch {
      return false;
    }
  }
  return false;
}
function getMimeType2(ext) {
  const mime = {
    ".html": "text/html",
    ".js": "application/javascript",
    ".css": "text/css",
    ".json": "application/json",
    ".png": "image/png",
    ".svg": "image/svg+xml",
    ".ico": "image/x-icon"
  };
  return mime[ext] || "application/octet-stream";
}
function sanitizePath(moduleId, filePath) {
  const cleanPath = filePath.split("?")[0];
  const normalized = path3.normalize(cleanPath);
  if (normalized.startsWith("..") || normalized.includes("..") || normalized.includes("../") || normalized.includes("..\\")) {
    return null;
  }
  const moduleRoot = path3.resolve(getModulesDir(), moduleId);
  const fullPath = path3.resolve(moduleRoot, normalized);
  if (!fullPath.startsWith(moduleRoot + path3.sep) && fullPath !== moduleRoot) {
    return null;
  }
  return fullPath;
}
var sdkScript = null;
function getSdkScript() {
  if (sdkScript) return sdkScript;
  const candidates = [
    SDK_PATH,
    path3.join(__dirname, "lib", "bridge-sdk.js"),
    path3.join(__dirname, "..", "..", "src", "lib", "bridge-sdk.js")
  ];
  for (const candidate of candidates) {
    try {
      sdkScript = fs3.readFileSync(candidate, "utf-8");
      return sdkScript;
    } catch {
    }
  }
  sdkScript = 'console.error("[Natives SDK] Failed to load bridge-sdk.js");';
  return sdkScript;
}
function readBody(req) {
  return new Promise((resolve3, reject) => {
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
    });
    req.on("end", () => resolve3(body));
    req.on("error", reject);
  });
}
async function handleRequest(req, res) {
  if (!validateHost(req.headers.host)) {
    res.writeHead(403);
    res.end("Forbidden");
    return;
  }
  if (!validateOrigin(req.method, req.headers.origin, req.headers.referer)) {
    res.writeHead(403);
    res.end("Forbidden");
    return;
  }
  res.setHeader("Content-Security-Policy", "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src http://localhost:* https:; frame-src 'self' https:; frame-ancestors 'none'; form-action 'none'");
  const url = new URL(req.url || "/", `http://localhost:${activePort}`);
  const pathname = decodeURIComponent(url.pathname);
  if (req.method === "GET" && pathname === "/natives-sdk.js") {
    res.writeHead(200, { "Content-Type": "application/javascript" });
    res.end(getSdkScript());
    return;
  }
  const moduleMatch = pathname.match(/^\/modules\/([^/]+)\/(.+)$/);
  if (req.method === "GET" && moduleMatch) {
    const moduleId = moduleMatch[1];
    const filePath = moduleMatch[2];
    const safePath = sanitizePath(moduleId, filePath);
    if (!safePath) {
      res.writeHead(403);
      res.end("Forbidden");
      return;
    }
    if (!fs3.existsSync(safePath)) {
      res.writeHead(404);
      res.end("Not Found");
      return;
    }
    const ext = path3.extname(safePath);
    res.writeHead(200, { "Content-Type": getMimeType2(ext) });
    res.end(fs3.readFileSync(safePath));
    return;
  }
  const bridgeMatch = pathname.match(/^\/api\/bridge\/([^/]+)\/([^/]+)$/);
  if (req.method === "POST" && bridgeMatch) {
    const namespace = bridgeMatch[1];
    const method = bridgeMatch[2];
    const token = req.headers["x-session-token"];
    const moduleId = req.headers["x-module-id"];
    if (verifyToken) {
      if (!token || !moduleId) {
        res.writeHead(401, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: "Missing X-Session-Token or X-Module-Id header" }));
        return;
      }
      if (!verifyToken(moduleId, token)) {
        res.writeHead(403, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: "Invalid session token" }));
        return;
      }
    }
    try {
      const body = await readBody(req);
      const parsed = body ? JSON.parse(body) : {};
      if (bridgeHandler) {
        const result = await bridgeHandler(namespace, method, moduleId || "", parsed);
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify(result));
      } else {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ ok: true, namespace, method, data: parsed }));
      }
    } catch (err) {
      res.writeHead(400, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: "Invalid JSON or handler error" }));
    }
    return;
  }
  if (req.method === "GET" && pathname === "/api/fs/raw") {
    const filePath = url.searchParams.get("path");
    if (!filePath) {
      res.writeHead(400);
      res.end("Missing path parameter");
      return;
    }
    if (filePath.includes("\0") || filePath.includes("..")) {
      res.writeHead(403);
      res.end("Forbidden");
      return;
    }
    try {
      const stat = await fs3.promises.stat(filePath);
      if (!stat.isFile()) {
        res.writeHead(400);
        res.end("Path is a directory");
        return;
      }
      if (isHeic(filePath)) {
        const jpegPath = await serveHeicAsJpegPath(filePath, stat);
        if (jpegPath) {
          const jpegStat = await fs3.promises.stat(jpegPath);
          const stream = fs3.createReadStream(jpegPath);
          res.writeHead(200, {
            "Content-Type": "image/jpeg",
            "Content-Length": String(jpegStat.size),
            "Cache-Control": "public, max-age=604800"
          });
          stream.pipe(res);
          stream.on("error", () => {
            if (!res.headersSent) {
              res.writeHead(500);
              res.end("Transcode error");
            }
          });
          return;
        }
        res.writeHead(415);
        res.end("HEIC transcode failed");
        return;
      }
      const rangeHeader = req.headers.range;
      let range;
      if (rangeHeader) {
        const match = rangeHeader.match(/bytes=(\d+)-(\d*)/);
        if (match) {
          range = { start: parseInt(match[1], 10) };
          if (match[2] !== "") range.end = parseInt(match[2], 10);
        }
      }
      const result = await streamFile(filePath, range);
      res.writeHead(range ? 206 : 200, {
        "Content-Type": result.contentType,
        "Accept-Ranges": "bytes",
        "Content-Length": range ? String(result.totalSize - (range.start || 0)) : String(result.totalSize),
        "Cache-Control": "no-cache",
        ...result.contentRange ? { "Content-Range": result.contentRange } : {}
      });
      result.stream.pipe(res);
      result.stream.on("error", () => {
        if (!res.headersSent) {
          res.writeHead(500);
          res.end("Internal Server Error");
        }
      });
    } catch (err) {
      if (err?.code === "ENOENT") {
        res.writeHead(404);
        res.end("File not found");
      } else if (err?.code === "EISDIR") {
        res.writeHead(400);
        res.end("Path is a directory");
      } else {
        res.writeHead(500);
        res.end("Internal Server Error");
      }
    }
    return;
  }
  if (req.method === "GET" && pathname === "/api/fs/thumb") {
    const filePath = url.searchParams.get("path");
    const width = parseInt(url.searchParams.get("w") || "320", 10);
    if (!filePath) {
      res.writeHead(400);
      res.end("Missing path parameter");
      return;
    }
    if (filePath.includes("\0") || filePath.includes("..")) {
      res.writeHead(403);
      res.end("Forbidden");
      return;
    }
    try {
      const { generateThumb: generateThumb2 } = await Promise.resolve().then(() => (init_thumbnail(), thumbnail_exports));
      const result = await generateThumb2(filePath, width);
      if (result) {
        res.writeHead(200, {
          "Content-Type": result.contentType,
          "Content-Length": String(result.buffer.length),
          "Cache-Control": "public, max-age=604800"
        });
        res.end(result.buffer);
      } else {
        res.writeHead(415);
        res.end("Cannot generate thumbnail");
      }
    } catch {
      res.writeHead(500);
      res.end("Thumbnail generation failed");
    }
    return;
  }
  if (req.method === "POST" && pathname === "/api/fs/copy") {
    const src = url.searchParams.get("src");
    const dir = url.searchParams.get("dir");
    if (!src || !dir) {
      res.writeHead(400, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: "Missing src or dir parameter" }));
      return;
    }
    if (src.includes("\0") || dir.includes("\0") || dir.includes("..")) {
      res.writeHead(403);
      res.end("Forbidden");
      return;
    }
    try {
      await fs3.promises.mkdir(dir, { recursive: true });
      const name = path3.basename(src);
      const target = path3.join(dir, name);
      await fs3.promises.copyFile(src, target);
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ ok: true, path: target }));
    } catch (err) {
      res.writeHead(500, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: err?.message || "Copy failed" }));
    }
    return;
  }
  if (req.method === "POST" && pathname === "/api/fs/save") {
    const dir = url.searchParams.get("dir");
    const name = url.searchParams.get("name");
    if (!dir || !name) {
      res.writeHead(400, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: "Missing dir or name parameter" }));
      return;
    }
    if (dir.includes("\0") || dir.includes("..") || name.includes("\0") || name.includes("/") || name.includes("\\")) {
      res.writeHead(403);
      res.end("Forbidden");
      return;
    }
    try {
      await fs3.promises.mkdir(dir, { recursive: true });
      const target = path3.join(dir, name);
      const chunks = [];
      await new Promise((resolve3, reject) => {
        req.on("data", (chunk) => chunks.push(Buffer.from(chunk)));
        req.on("end", resolve3);
        req.on("error", reject);
      });
      const body = Buffer.concat(chunks);
      await fs3.promises.writeFile(target, body);
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ ok: true, path: target, size: body.length }));
    } catch (err) {
      res.writeHead(500, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: err?.message || "Write failed" }));
    }
    return;
  }
  res.writeHead(404);
  res.end("Not Found");
}
function startServer(port) {
  return new Promise((resolve3, reject) => {
    if (server) {
      resolve3(activePort);
      return;
    }
    server = http.createServer((req, res) => {
      handleRequest(req, res).catch((err) => {
        console.error("[HTTP Server] Unhandled error:", err);
        if (!res.headersSent) {
          res.writeHead(500);
          res.end("Internal Server Error");
        }
      });
    });
    const listenPort = port || 0;
    server.listen(listenPort, () => {
      const addr = server.address();
      if (addr && typeof addr === "object") {
        activePort = addr.port;
        resolve3(activePort);
      } else {
        reject(new Error("Failed to get server address"));
      }
    });
    server.on("error", reject);
  });
}
function stopServer() {
  if (server) {
    server.close();
    server = null;
    activePort = 0;
  }
}
function getPort() {
  return activePort;
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  getPort,
  setBridgeHandler,
  setTokenVerifier,
  startServer,
  stopServer,
  validateHost,
  validateOrigin
});
