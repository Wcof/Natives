# ADR-0008: Electron to Tauri v2 Migration

## Status

Accepted

## Context

Natives was originally built with Electron 34 as the desktop container. Electron provides a full Chromium + Node.js runtime, but has significant drawbacks:

- **Bundle size**: Electron ships ~150MB with every app (Chromium + Node.js)
- **Memory usage**: Each Electron app runs its own Chromium instance (~200-400MB baseline)
- **Security surface**: Node.js integration in renderer, IPC bridge complexity
- **Platform packaging**: electron-builder configuration complexity

Tauri v2 offers an alternative approach:
- Uses the system WebView (WebKit on macOS, WebView2 on Windows) — no bundled browser engine
- Rust backend replaces Node.js main process — memory-safe, smaller binary
- Native IPC between WebView and Rust — no Node.js bridge needed
- Smaller bundle size (~5-10MB)
- Built-in security model with capability-based permissions

## Decision

Migrate Natives from Electron to Tauri v2.

## Consequences

### Positive
- **Bundle size**: ~10MB vs ~150MB (90% reduction)
- **Memory**: ~50-100MB vs ~200-400MB (70% reduction)
- **Security**: Rust backend with explicit capability permissions vs Node.js with contextIsolation
- **Performance**: Rust I/O operations (file, DB, PTY) are faster than Node.js equivalents
- **Type safety**: Rust's type system catches more errors at compile time

### Negative
- **WebView differences**: System WebView may render slightly differently than Chromium
- **Rust learning curve**: Contributors need Rust knowledge for backend changes
- **Plugin ecosystem**: Tauri plugin ecosystem is smaller than Electron's npm ecosystem
- **macOS-specific**: Some features (screenshot detection, trash) need macOS-specific implementations

### Neutral
- **Frontend unchanged**: React/Next.js frontend remains identical — only the container changes
- **IPC contract preserved**: `window.nativesAPI` contract is maintained via Tauri adapter
- **Database unchanged**: SQLite with same schema, same WAL mode, same 10 tables

## Implementation Notes

### Replaced Components
| Electron | Tauri v2 |
|----------|----------|
| `electron/main.ts` | `src-tauri/src/lib.rs` + `src-tauri/src/commands/*.rs` |
| `electron/preload.ts` | `src/lib/tauri-adapter.ts` |
| `node-pty` | `portable-pty` (Rust crate) |
| `electron.safeStorage` | AES-256-GCM (`src-tauri/src/env_manager.rs`) |
| `electron-builder` | `tauri build` |
| `BrowserWindow` | `tauri::WebviewWindow` |
| `ipcMain.handle` | `#[tauri::command]` |
| `ipcRenderer.invoke` | `invoke()` from `@tauri-apps/api/core` |
| `ipcRenderer.on` | `listen()` from `@tauri-apps/api/event` |

### Preserved Architecture
- Three-layer architecture (Application → Framework → Service → Infrastructure)
- Four design pillars (Zero-Code Embedding, Style Self-Definition, Environment Injection, Sub-Application Isolation)
- Five defense lines (Watchdog, iframe sandbox, PTY, DB bus, FOUC guard)
- `window.nativesAPI` contract with all 80+ methods
- 10-table SQLite schema with WAL mode and foreign keys

### Security Model Changes
- **iframe sandbox**: Preserved exactly (`allow-scripts allow-forms`, no `allow-same-origin`)
- **Session tokens**: Moved from Node.js crypto to Rust HMAC-SHA256
- **Local HTTP server**: Moved from Node.js http to Rust tiny_http
- **CSP**: Same policy, now enforced by both Tauri and the HTTP server
- **Path validation**: Same allowlist/blocklist, now in Rust
