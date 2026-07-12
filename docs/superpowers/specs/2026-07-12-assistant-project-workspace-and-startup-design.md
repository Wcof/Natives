# Assistant Project Workspace and Startup Design

## Context

Natives already contains an Assistant V2 page, a Tauri-managed `natives-agent-daemon`, and persisted provider/model configuration. The current page treats conversations as a flat list filtered by one locally stored project path. Its development command starts only the Next.js renderer, so opening the page through `npm run dev` has no Tauri bridge or daemon supervisor and reports that the assistant module is unavailable.

CodePilot provides the relevant interaction reference: project folders are first-class groups in the left rail and conversations live directly under their working-directory project. This design adopts that pattern without copying CodePilot's web API architecture or bypassing Natives' Tauri IPC boundary.

## Product Model

The Assistant has three explicit parts:

| Part | Responsibility | Source of truth |
| --- | --- | --- |
| Page | Project directory navigation, conversations, messages, and actionable state feedback | Assistant V2 RPC via `window.nativesAPI` |
| Engine | Starts, supervises, restarts, and serves the local assistant daemon | Tauri `DaemonSupervisor` and daemon socket |
| Provider | Stores encrypted keys and real discovered models, and supplies a usable model to a run | Assistant provider tables and model cache |

A page is ready for conversation creation only when all three are ready: the engine handshake succeeds, at least one provider has an active key, and that provider has a discovered model.

## Workspace and Conversation Interaction

### Project directory is the only first-level group

The left rail has one project list. Every visible project is a real directory identified by its canonical project path (`project_id`); there is no intermediate “conversation” directory or a synthetic folder hierarchy.

Each project row contains:

- Folder name and a compact, non-secret path tooltip.
- Expand/collapse control persisted for the current desktop session.
- New conversation action that creates a Chat or Agent conversation in that exact project.
- Project actions: choose/open a directory, copy path, and remove the project grouping only through an explicit destructive confirmation that explains it archives/removes its conversations.

The project list is ordered by most recent conversation activity. A project expands on first use when it is the active project; its conversations are direct children sorted by `updated_at` descending.

### Conversation rows

Each conversation row shows title, mode, updated time, active/streaming state, and an overflow menu for rename, archive, and delete. The page never displays fake conversations or fake project names.

Conversations without `project_id` appear under a clearly named localized “Unassigned” group. This preserves existing data while the user can select a real directory for newly created conversations.

### Directory selection and creation

The “New project” command opens the existing native directory picker through the Tauri adapter. Selecting a directory makes it active and opens/creates its group. Creating Chat or Agent then selects a provider/model through the provider readiness contract and sends that directory path as `project_id` to `conversation.create`.

The active project remains in application state and is persisted using the existing settings/adapter path rather than a feature-local browser-only convention. On launch, the last active project is restored if it still exists; otherwise the page opens the project chooser and does not invent a replacement.

## Engine Startup and Recovery

### Development command contract

`npm run dev` becomes the supported desktop development entry point:

1. Build the `natives-agent-daemon` debug sidecar.
2. Start `tauri dev`.
3. Tauri starts `web:dev` (the renamed Next.js-only command) through `beforeDevCommand`.
4. Tauri constructs `DaemonSupervisor`, starts the sidecar, completes the authenticated handshake, and exposes the status through the existing Assistant V2 adapter.

`web:dev` remains available for renderer-only work, but the Assistant page must show an explicit “desktop engine required” development state rather than a generic unavailable message if it is opened without Tauri.

Production build scripts prepare the release sidecar before invoking Tauri packaging. The daemon path resolver must search the development target path and the packaged sidecar location, and diagnostics must report the selected path and startup failure category without leaking tokens or provider keys.

### Page-level startup states

The page renders the following distinct states:

- **Connecting engine:** handshake is in progress; show an inline loading state.
- **Engine unavailable:** no desktop bridge, missing binary, failed spawn, failed handshake, or exhausted restart attempts. Show the safe classified reason, a Retry action that actually retries engine status/startup, and a development hint when renderer-only mode is detected.
- **Provider setup needed:** engine is ready but no active provider key exists. Keep directory browsing available and direct the user to provider settings.
- **Model setup needed:** a provider exists but has no discovered model. Direct the user to refresh models in provider settings.
- **Ready:** enable project and conversation actions.

Configuration errors must not be labelled as engine failures, and engine errors must not be labelled as provider failures.

## Data and IPC Contract

The renderer uses only `window.nativesAPI`. It receives project-grouped data through one Assistant V2 RPC response or a deterministic client grouping of `conversation.list` results; it does not fetch the daemon socket or SQLite directly.

The backend conversation list accepts an optional project filter for focused loading and additionally supports listing all non-archived conversations for project grouping. The response retains `project_id`, `updated_at`, mode, and runtime status needed by the page.

Directory identity is the absolute path stored in `project_id`. Display labels derive from the final path segment; no editable duplicate project-name table is introduced in this iteration. This matches the user’s “directory is project” requirement while avoiding a second identity source.

Provider selection remains independent of projects. A conversation persists the provider/model selected at creation time; a new conversation chooses the first valid default using the existing discovered-model selection rule.

## Error Handling and Security

- All user-facing errors pass through `classifyError` and localized messages.
- Provider API keys and daemon bootstrap/session tokens never reach the page.
- The directory picker result is handled by Tauri; renderer code does not access filesystem APIs directly.
- No project, conversation, model, or health status is fabricated while loading or after an error.
- Existing encrypted credential storage and daemon socket authentication remain unchanged.

## Testing Strategy

Test-first coverage includes:

1. Pure grouping: conversations are grouped by exact project path, unassigned conversations form one group, and ordering follows most recent update.
2. Project selection: valid directory selection becomes the creation `project_id`; no directory prevents creation without displaying a false engine error.
3. Conversation model selection: creation receives a valid active provider/model only.
4. Engine status mapping: renderer-only, connecting, spawn failure, handshake failure, provider needed, model needed, and ready are distinct.
5. Development scripts: `npm run dev` builds the sidecar and Tauri config starts only `web:dev` as its frontend command.
6. Supervisor path resolution: debug daemon binary is found after the development build, and missing binary yields a classified diagnostic.
7. Existing daemon/provider/conversation regression tests plus TypeScript type checking, ESLint, Cargo tests, and Cargo check.

## Acceptance Criteria

- Assistant left rail shows only top-level real project directories, with conversations directly beneath each project.
- New Chat and New Agent are created in the selected directory/project and retain that association.
- Existing project-less conversations are visible under a localized Unassigned group.
- The page visibly distinguishes engine startup/failure from provider/model setup.
- `npm run dev` starts the desktop app and its daemon sidecar; a new Assistant page reaches engine-ready state without manual sidecar setup.
- Renderer-only `web:dev` explains the missing desktop engine accurately.
- Provider/model readiness continues to use saved real discovered models.
- No renderer direct database/socket/filesystem access or credential leakage is introduced.
