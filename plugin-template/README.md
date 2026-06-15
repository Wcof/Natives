# Natives Plugin Template

This is a template for creating Natives plugins. A plugin is a standalone HTML/JS/CSS page that runs inside Natives' sandboxed iframe.

## Getting Started

1. Copy this directory to `~/.natives/modules/your-plugin-id/`
2. Edit `manifest.json` with your plugin details
3. Edit `index.html` to build your plugin
4. Restart Natives or click "Scan Modules" in the Workshop

## Plugin Structure

```
your-plugin/
├── manifest.json    # Plugin metadata and permissions
├── index.html       # Entry point
├── style.css        # Styles (optional)
├── app.js           # Application logic (optional)
└── icon.png         # Plugin icon (optional)
```

## Bridge API

Your plugin communicates with Natives through `window.natives.*`:

### Data Storage
```javascript
await window.natives.db.set('key', 'value');
const value = await window.natives.db.get('key');
await window.natives.db.delete('key');
const keys = await window.natives.db.list('prefix');
```

### Settings
```javascript
const theme = await window.natives.settings.getTheme();
const locale = await window.natives.settings.getLocale();
window.natives.settings.onThemeChange((theme) => { /* ... */ });
```

### Lifecycle
```javascript
// Tell Natives you're ready
window.natives.lifecycle.ready();

// Respond to heartbeat
window.natives.lifecycle.onHeartbeat(() => {
  // Acknowledge heartbeat (no return value needed)
});

// Save state before unload
window.natives.lifecycle.onUnload(async () => {
  await window.natives.db.set('my-state', JSON.stringify(state));
});

// Report errors
window.natives.lifecycle.error({ message: 'Something went wrong' });
```

### Notifications (requires `notification` permission)
```javascript
await window.natives.notification.send('Title', 'Body');
await window.natives.notification.badge(3);
```

### IPC (requires `ipc:send` permission)
```javascript
await window.natives.ipc.send('com.other.plugin', { action: 'hello' });
window.natives.ipc.on('channel', (data, sender) => { /* ... */ });
await window.natives.ipc.broadcast('channel', { message: 'hello all' });
```

### Meta
```javascript
window.natives.meta.moduleId;      // Your plugin ID
window.natives.meta.nativesVersion; // Natives version
```

## Manifest Reference

| Field | Required | Description |
|-------|----------|-------------|
| `id` | ✅ | Unique identifier (reverse domain recommended) |
| `name` | ✅ | Display name |
| `version` | ✅ | SemVer version |
| `entry` | ✅ | Entry HTML file path |
| `type` | ✅ | Module type (`"web"` for MVP) |
| `permissions` | ✅ | Required permissions list |
| `description` | ❌ | Plugin description |
| `author` | ❌ | Author name |
| `icon` | ❌ | Icon file path |
| `minNativesVersion` | ❌ | Minimum Natives version |
| `lifecycle.heartbeatInterval` | ❌ | Heartbeat interval (ms), default 5000 |
| `lifecycle.loadTimeout` | ❌ | Load timeout (ms), default 10000 |

## Permissions

- `db:read` — Read data from module_data store
- `db:write` — Write data to module_data store
- `notification` — Send notifications
- `ipc:send` — Send messages to other plugins
- `ipc:receive` — Receive messages from other plugins
- `env:read` — Read environment variables
