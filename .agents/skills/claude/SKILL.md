---
name: claude_rules
description: Custom workspace rules from CLAUDE.md, including mandatory rtk command prefixing and CodeGraph code graph navigation.
---

# CLAUDE.md Workspace Skill Rules

Please strictly adhere to the following rules loaded from [CLAUDE.md](file:///Users/ldh/Downloads/project/AiNative/Natives2/CLAUDE.md):

## 1. Mandatory RTK Command Prefix
All shell commands proposed or executed in this repository MUST be prefixed with `rtk`.
- **Correct**: `rtk npm run build`, `rtk git diff`, `rtk npm test`
- **Incorrect**: `npm run build`, `git diff`, `npm test`

## 2. CodeGraph Exploration First
Since the `.codegraph/` directory exists in the workspace root, code graph exploration must be used:
- Prioritize CodeGraph MCP tools (`codegraph_explore`, `codegraph_node`) or shell commands (`rtk codegraph explore "<query>"`, `rtk codegraph node <symbol>`) over standard grep/ripgrep or reading raw files when trying to locate definitions or understand call graphs.

# DESKTOP APPLICATION OVERRIDES (HIGHEST PRIORITY)
1. DO NOT use heavy scroll-based trigger animations (ScrollTrigger). The layout must be a rigid dashboard/workspace layout fitting the window viewport.
2. Maintain a strict layout hierarchy: Sidebar (fixed width), Top Navbar (fixed height with custom window controls), and Main Workspace (scrollable content).
3. Window Drag Region: Ensure the custom title bar component strictly retains `-webkit-app-region: drag`, and all clickable buttons inside it have `drag-none`.
4. Performance Rule: Limit the use of CSS `backdrop-filter: blur` on dynamic, high-refreshing elements to prevent frame drops in the Tauri/Electron Webview renderer.