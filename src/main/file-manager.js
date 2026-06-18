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

// src/main/file-manager.ts
var file_manager_exports = {};
__export(file_manager_exports, {
  createEntry: () => createEntry,
  importFiles: () => importFiles,
  listDir: () => listDir,
  moveEntry: () => moveEntry,
  readFile: () => readFile,
  renameEntry: () => renameEntry,
  streamFile: () => streamFile,
  trashEntry: () => trashEntry,
  writeFileAtomic: () => writeFileAtomic
});
module.exports = __toCommonJS(file_manager_exports);
var fs = __toESM(require("fs"));
var path = __toESM(require("path"));
var os = __toESM(require("os"));
var import_child_process = require("child_process");

// src/types/file.ts
var TEXT_EXTENSIONS = /* @__PURE__ */ new Set([
  ".txt",
  ".md",
  ".markdown",
  ".mdown",
  ".ts",
  ".tsx",
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
  ".json",
  ".jsonc",
  ".yaml",
  ".yml",
  ".toml",
  ".xml",
  ".css",
  ".scss",
  ".sass",
  ".less",
  ".styl",
  ".html",
  ".htm",
  ".xhtml",
  ".py",
  ".pyw",
  ".rb",
  ".php",
  ".java",
  ".kt",
  ".kts",
  ".scala",
  ".rs",
  ".go",
  ".mod",
  ".sum",
  ".c",
  ".h",
  ".cpp",
  ".hpp",
  ".cc",
  ".hh",
  ".cs",
  ".swift",
  ".m",
  ".mm",
  ".sh",
  ".bash",
  ".zsh",
  ".fish",
  ".ps1",
  ".bat",
  ".env",
  ".gitignore",
  ".dockerignore",
  ".editorconfig",
  ".vue",
  ".svelte",
  ".astro",
  ".sql",
  ".graphql",
  ".gql",
  ".conf",
  ".ini",
  ".cfg"
]);
var IMAGE_EXTENSIONS = /* @__PURE__ */ new Set([".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".ico", ".bmp", ".tiff", ".tif", ".avif"]);
var VIDEO_EXTENSIONS = /* @__PURE__ */ new Set([".mp4", ".mov", ".avi", ".mkv", ".webm", ".m4v", ".wmv", ".flv"]);
var AUDIO_EXTENSIONS = /* @__PURE__ */ new Set([".mp3", ".wav", ".flac", ".ogg", ".m4a", ".aac", ".wma", ".opus"]);
var ARCHIVE_EXTENSIONS = /* @__PURE__ */ new Set([".zip", ".tar.gz", ".tar.bz2", ".tar.xz", ".tar", ".gz", ".bz2", ".xz", ".rar", ".7z", ".tgz", ".tbz2"]);
var EXTENSIONLESS_TEXT_FILES = /* @__PURE__ */ new Set([
  "Dockerfile",
  "Makefile",
  "Gemfile",
  "Rakefile",
  "CHANGELOG",
  "README",
  "LICENSE",
  "VERSION",
  "Procfile",
  ".env",
  ".gitignore"
]);
function detectFileKind(fileName) {
  if (EXTENSIONLESS_TEXT_FILES.has(fileName)) return "text";
  const dotIndex = fileName.lastIndexOf(".");
  if (dotIndex === -1) return "other";
  const ext = fileName.slice(dotIndex).toLowerCase();
  const secondDot = fileName.lastIndexOf(".", dotIndex - 1);
  if (secondDot !== -1) {
    const compoundExt = fileName.slice(secondDot).toLowerCase();
    if (ARCHIVE_EXTENSIONS.has(compoundExt)) return "archive";
  }
  if (TEXT_EXTENSIONS.has(ext)) return "text";
  if (IMAGE_EXTENSIONS.has(ext)) return "image";
  if (VIDEO_EXTENSIONS.has(ext)) return "video";
  if (AUDIO_EXTENSIONS.has(ext)) return "audio";
  if (ext === ".pdf") return "pdf";
  if (ARCHIVE_EXTENSIONS.has(ext)) return "archive";
  return "other";
}
function detectProjectBadge(_dirPath, fileNames, hasGitDir) {
  const nameSet = new Set(fileNames.map((f) => f.toLowerCase()));
  if (nameSet.has("package.json")) return "node";
  if (nameSet.has("index.html")) return "web";
  if (nameSet.has("requirements.txt") || nameSet.has("setup.py") || nameSet.has("pyproject.toml")) return "python";
  if (nameSet.has("cargo.toml")) return "rust";
  if (nameSet.has("go.mod")) return "go";
  if (hasGitDir) return "git";
  return void 0;
}

// src/main/file-manager.ts
function expandTilde(p) {
  if (p === "~" || p.startsWith("~/") || p.startsWith("~\\")) {
    return path.join(os.homedir(), p.slice(1));
  }
  return p;
}
var DS_STORE = ".DS_Store";
async function listDir(dirPath, options) {
  dirPath = expandTilde(dirPath);
  const stat = await fs.promises.stat(dirPath);
  if (!stat.isDirectory()) {
    throw Object.assign(new Error(`Not a directory: ${dirPath}`), { code: "ENOTDIR" });
  }
  const entries = await fs.promises.readdir(dirPath);
  const filtered = entries.filter((name) => {
    if (name === DS_STORE) return false;
    if (name.startsWith(".") && !options?.showHidden) return false;
    return true;
  });
  const IO_BATCH = 64;
  const result = [];
  async function processEntry(name) {
    const fullPath = path.resolve(dirPath, name);
    try {
      const lstat = await fs.promises.lstat(fullPath);
      const isSymlink = lstat.isSymbolicLink();
      let symlinkTarget;
      let targetStat;
      if (isSymlink) {
        symlinkTarget = await fs.promises.readlink(fullPath);
        try {
          targetStat = await fs.promises.stat(fullPath);
        } catch {
          targetStat = lstat;
        }
      } else {
        targetStat = lstat;
      }
      const isDir = targetStat.isDirectory();
      const kind = isDir ? "other" : detectFileKind(name);
      let projectBadge;
      if (isDir && entries.length <= 80) {
        try {
          const childFiles = await fs.promises.readdir(fullPath);
          const hasGit = childFiles.includes(".git");
          projectBadge = detectProjectBadge(fullPath, childFiles, hasGit);
        } catch {
        }
      }
      return {
        name,
        path: fullPath,
        isDir,
        kind,
        hidden: name.startsWith("."),
        size: isDir ? 4096 : targetStat.size,
        mtime: targetStat.mtimeMs,
        btime: targetStat.birthtimeMs,
        ...symlinkTarget ? { symlink: symlinkTarget } : {},
        ...projectBadge ? { projectBadge } : {}
      };
    } catch {
      return null;
    }
  }
  for (let i = 0; i < filtered.length; i += IO_BATCH) {
    const batch = filtered.slice(i, i + IO_BATCH);
    const batchResults = await Promise.all(batch.map(processEntry));
    for (const entry of batchResults) {
      if (entry) result.push(entry);
    }
  }
  const sortBy = options?.sortBy || "name";
  const sortDir = options?.sortDir || "asc";
  result.sort((a, b) => {
    let cmp;
    switch (sortBy) {
      case "mtime":
        cmp = a.mtime - b.mtime;
        break;
      case "size":
        cmp = a.size - b.size;
        break;
      case "name":
      default:
        cmp = a.name.localeCompare(b.name, "zh-CN", { numeric: true });
        break;
    }
    return sortDir === "desc" ? -cmp : cmp;
  });
  return result;
}
var MAX_FULL_READ = 2 * 1024 * 1024;
var MAX_TRUNCATED_READ = 256 * 1024;
async function readFile(filePath) {
  filePath = expandTilde(filePath);
  const stat = await fs.promises.stat(filePath);
  if (!stat.isFile()) {
    throw Object.assign(new Error(`Not a file: ${filePath}`), { code: "EISDIR" });
  }
  const fileSize = stat.size;
  const encoding = "utf-8";
  const truncated = fileSize > MAX_FULL_READ;
  let content;
  if (truncated) {
    const buffer = Buffer.alloc(Math.min(MAX_TRUNCATED_READ, fileSize));
    const fd = await fs.promises.open(filePath, "r");
    try {
      await fd.read(buffer, 0, buffer.length, 0);
    } finally {
      await fd.close();
    }
    content = buffer.toString(encoding).replace(/\0+$/, "");
  } else {
    content = await fs.promises.readFile(filePath, encoding);
  }
  return { content, truncated, size: fileSize, encoding };
}
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
async function writeFileAtomic(filePath, content, expectedMtime) {
  filePath = expandTilde(filePath);
  const dir = path.dirname(filePath);
  if (expectedMtime !== void 0) {
    try {
      const stat = await fs.promises.stat(filePath);
      const actualMtime = stat.mtimeMs;
      if (Math.abs(actualMtime - expectedMtime) > 1) {
        return { mtime: actualMtime, conflict: true };
      }
    } catch (err) {
      if (err.code !== "ENOENT") throw err;
    }
  }
  const tmpFile = path.join(
    dir,
    `.tmp-${path.basename(filePath)}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  );
  try {
    const fd = await fs.promises.open(tmpFile, "wx");
    try {
      await fd.writeFile(content, "utf-8");
      await fd.sync();
    } finally {
      await fd.close();
    }
    await fs.promises.rename(tmpFile, filePath);
    const newStat = await fs.promises.stat(filePath);
    return { mtime: newStat.mtimeMs, conflict: false };
  } catch (err) {
    try {
      await fs.promises.unlink(tmpFile);
    } catch {
    }
    throw err;
  }
}
var ALLOWED_ROOTS = [
  os.homedir(),
  "/tmp",
  "/private/tmp"
];
function validatePath(targetPath) {
  if (targetPath.includes("\0")) {
    throw Object.assign(new Error(`Path contains null byte: ${targetPath}`), { code: "EINVAL" });
  }
  const expanded = expandTilde(targetPath);
  const resolved = path.resolve(expanded);
  const isAllowed = ALLOWED_ROOTS.some((root) => {
    const resolvedRoot = path.resolve(root);
    return resolved === resolvedRoot || resolved.startsWith(resolvedRoot + path.sep);
  });
  if (!isAllowed) {
    throw Object.assign(
      new Error(`Path outside allowed roots: ${targetPath} (resolved: ${resolved})`),
      { code: "EACCES" }
    );
  }
  const relativeToHome = path.relative(os.homedir(), resolved);
  const sensitiveDirs = [".ssh", ".gnupg", ".aws", ".config/gh", ".kube"];
  for (const sensitive of sensitiveDirs) {
    if (relativeToHome === sensitive || relativeToHome.startsWith(sensitive + path.sep)) {
      throw Object.assign(
        new Error(`Access to sensitive directory denied: ${targetPath}`),
        { code: "EACCES" }
      );
    }
  }
}
async function createEntry(targetPath, type) {
  validatePath(targetPath);
  if (type === "dir") {
    await fs.promises.mkdir(targetPath, { recursive: false });
  } else {
    const fd = await fs.promises.open(targetPath, "wx");
    await fd.close();
  }
}
async function deduplicatePath(targetPath) {
  try {
    await fs.promises.access(targetPath);
  } catch {
    return targetPath;
  }
  const dir = path.dirname(targetPath);
  const ext = path.extname(targetPath);
  const base = path.basename(targetPath, ext);
  for (let counter = 1; counter < 100; counter++) {
    const candidate = path.join(dir, `${base} (${counter})${ext}`);
    try {
      await fs.promises.access(candidate);
    } catch {
      return candidate;
    }
  }
  return targetPath;
}
async function renameEntry(oldPath, newPath) {
  validatePath(oldPath);
  validatePath(newPath);
  await fs.promises.stat(oldPath);
  const targetPath = await deduplicatePath(newPath);
  await fs.promises.rename(oldPath, targetPath);
}
async function trashEntry(filePath) {
  validatePath(filePath);
  await fs.promises.stat(filePath);
  const script = `
    use framework "Foundation"
    set fileURL to current application's NSURL's fileURLWithPath:"${filePath.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"
    set workspace to current application's NSWorkspace's sharedWorkspace()
    workspace's recycleURLs:{fileURL} completionHandler:(missing value)
  `;
  await new Promise((resolve2, reject) => {
    (0, import_child_process.execFile)("osascript", ["-l", "AppleScript", "-e", script], { timeout: 1e4 }, (err, stdout, stderr) => {
      if (err) {
        (0, import_child_process.execFile)("osascript", [
          "-e",
          `POSIX file "${filePath.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}" as alias`,
          "-e",
          'tell application "Finder" to delete item 1 of result'
        ], { timeout: 1e4 }, (err2, stdout2, stderr2) => {
          if (err2) {
            reject(Object.assign(new Error(`Failed to trash: ${stderr2 || err2.message}`), { code: "ETRASH" }));
          } else {
            resolve2();
          }
        });
      } else {
        resolve2();
      }
    });
  });
}
async function moveEntry(from, to) {
  validatePath(from);
  validatePath(to);
  await fs.promises.stat(from);
  const targetPath = await deduplicatePath(to);
  try {
    await fs.promises.rename(from, targetPath);
  } catch (err) {
    if (err.code === "EXDEV") {
      const isDir = (await fs.promises.stat(from)).isDirectory();
      if (isDir) {
        await copyDir(from, targetPath);
      } else {
        await fs.promises.copyFile(from, targetPath);
      }
      await fs.promises.rm(from, { recursive: true, force: true });
    } else {
      throw err;
    }
  }
}
async function copyDir(src, dest) {
  await fs.promises.mkdir(dest, { recursive: true });
  const entries = await fs.promises.readdir(src, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);
    if (entry.isDirectory()) {
      await copyDir(srcPath, destPath);
    } else {
      await fs.promises.copyFile(srcPath, destPath);
    }
  }
}
async function importFiles(sourcePaths, destDir) {
  const results = [];
  for (const srcPath of sourcePaths) {
    const baseName = path.basename(srcPath);
    const destPath = await deduplicatePath(path.join(destDir, baseName));
    const stat = await fs.promises.stat(srcPath);
    if (stat.isDirectory()) {
      await copyDir(srcPath, destPath);
    } else {
      await fs.promises.copyFile(srcPath, destPath);
    }
    results.push(destPath);
  }
  return results;
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  createEntry,
  importFiles,
  listDir,
  moveEntry,
  readFile,
  renameEntry,
  streamFile,
  trashEntry,
  writeFileAtomic
});
