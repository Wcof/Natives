/**
 * Prompt Context Injector
 * ═══════════════════════════════════════════════════════════════════════
 * Injects the two supreme constraints (Visual Sovereignty + Bridge Protocol
 * Deadlock) into the prompt sent to the generator LLM. This guarantees every
 * piece of AI-generated HTML/JS/Tailwind code is:
 *
 *   1. Visually compliant — frosted glass, mirror-beveled borders,
 *      backdrop saturation matching macOS/visionOS aesthetic.
 *   2. Bridge-locked — no native fetch/XHR/global probes; only the injected
 *      `window.natives.*` proxy may be used, with JSON as the sole contract
 *      data carrier.
 *
 * The injector is pure (no side effects) and stateless — it can be called
 * from any context that needs to assemble a generation prompt.
 */

// ────────────────────────────────────────────────────────────────────────
// 1. 视觉主权控制 (macOS / visionOS UI Specification)
// ────────────────────────────────────────────────────────────────────────

export const VISUAL_SOVEREIGNTY_SPEC = `
## 视觉主权控制 (Visual Sovereignty — MUST comply)

生成 HTML 内部的 Tailwind / CSS 必须严格执行以下设计 Tokens：

### 1.1 高级毛玻璃与透光 (Frosted Glass)
- 主容器必须使用 \`backdrop-blur-md\` 或 \`backdrop-blur-xl\`。
- 背景色必须带有半透明度，例如 \`bg-[#0d0f12]/75\` 或项目内置 \`var(--bg-2)\`。
- 禁止使用不透明的实色背景作为顶层容器。

### 1.2 镜面斜边高光 border (Mirror Bevel)
- 卡片和边缘禁止使用实色边框。
- 必须使用 \`border border-white/[0.08]\` 或 \`border-t border-white/[0.15]\`，
  精确模拟物理反光与磨砂斜边。

### 1.3 折射与饱和度提升 (Refraction & Saturation)
- 搭配 \`backdrop-saturate-[1.8]\`，使容器下方的色彩元素产生自然的物理折射感。
- 顶层容器建议叠加多层：\`backdrop-blur-xl backdrop-saturate-[1.8]\`。

### 1.4 字体与间距 (Typography & Spacing)
- 使用系统字体栈：\`-apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif\`。
- 字符间距收紧：标题 \`tracking-tight\`，正文 \`tracking-normal\`。
- 圆角统一使用 \`rounded-xl\` 或 \`rounded-2xl\`，禁止直角矩形。

### 1.5 色彩继承 (CSS Variable Inheritance — MUST)
所有涉及前端增量刷新或提示的组件，必须 100% 继承项目已有的 CSS 变量系统：

- 背景：\`var(--bg-1)\`, \`var(--bg-2)\`, \`var(--bg-3)\`
- 前景：\`var(--fg-1)\`, \`var(--fg-2)\`, \`var(--fg-3)\`
- 强调：\`var(--accent)\`, \`var(--accent-soft)\`
- 边框：\`var(--border-1)\`, \`var(--border-2)\`
- 阴影：\`var(--shadow-glass)\`

禁止硬编码十六进制色值，必须引用 \`var(--*)\` 变量。
`.trim();

// ────────────────────────────────────────────────────────────────────────
// 2. 连接协议死锁 (Bridge Connection Rules)
// ────────────────────────────────────────────────────────────────────────

export const BRIDGE_PROTOCOL_SPEC = `
## 连接协议死锁 (Bridge Protocol Deadlock — MUST comply)

### 2.1 禁用原生网络 API (Native Network APIs — BANNED)
生成脚本中**绝对禁用**以下 API：
- \`fetch()\`
- \`XMLHttpRequest\`
- \`WebSocket\`
- \`navigator.sendBeacon()\`
- 任何对全局系统变量（\`window.opener\`, \`window.top\`, \`window.parent\` 除外，且仅用于 postMessage）的直接试探。

### 2.2 强制使用底座网桥代理 (Bridge Proxy — MANDATORY)
所有数据请求和 Skill 触发，必须且只能编写为调用底座注入在沙箱环境内的 \`window.natives.*\` 统一网桥代理。

可用方法：
\`\`\`js
// 数据读写（自动命名空间隔离到当前模块）
window.natives.db.get(key)        // → Promise<any>
window.natives.db.set(key, value) // → Promise<void>
window.natives.db.delete(key)     // → Promise<void>
window.natives.db.list(prefix)    // → Promise<string[]>

// 生命周期
window.natives.lifecycle.ready()  // 通知底座已就绪
window.natives.lifecycle.onHeartbeat(() => { /* 每 5s 调用 */ })

// 错误上报
window.natives.lifecycle.error({ message, stack })
\`\`\`

### 2.3 JSON 契约数据载体 (JSON Contract — MANDATORY)
所有请求与响应必须以 **JSON 对象**作为标准的契约数据载体。

- 请求体：\`{ "method": "db.get", "args": { "key": "..." }, "requestId": "..." }\`
- 响应体：\`{ "type": "bridge:response", "requestId": "...", "result": ... }\`

禁止使用 FormData、Blob、ArrayBuffer、纯字符串作为请求体。

### 2.4 数据命名空间隔离 (Namespace Isolation — MUST)
生成的代码无需关心命名空间前缀——底座内核会自动将所有 \`db.*\` 调用
重写为 \`custom_module_data_\${moduleId}/\${key}\`。

**生成的代码绝对无法越权触碰底座系统数据。**

### 2.5 会话 Token 自动注入 (Session Token — AUTO)
生成的代码**无需手动管理 Token**。底座在 iframe 加载完成后会通过
postMessage 自动下发 \`token-granted\` 消息。生成的代码只需：

\`\`\`js
window.addEventListener('message', (event) => {
  if (event.data?.type === 'token-granted') {
    window.__NATIVES_TOKEN__ = event.data.token;
    window.__NATIVES_MODULE_ID__ = event.data.moduleId;
  }
});
\`\`\`

或直接使用底座预注入的 \`window.natives.*\` 代理（已内置 Token 注入）。
`.trim();

// ────────────────────────────────────────────────────────────────────────
// 3. 完整提示词组装器 (Prompt Assembler)
// ────────────────────────────────────────────────────────────────────────

export interface GenerationPromptParams {
  /** User's natural language request, e.g. "我需要一个番茄钟应用". */
  userRequest: string;
  /** Available MCP/Skill descriptions the AI may compose. */
  availableSkills: string[];
  /** The unique module_id to be assigned (e.g. "com.ai.custom-tool-xyz"). */
  moduleId: string;
  /** Display name for the generated application. */
  moduleName: string;
}

/**
 * Assemble the full generation prompt with both supreme constraints injected.
 * This is the single entry point for the AI App Engine's prompt assembly.
 */
export function buildGenerationPrompt(params: GenerationPromptParams): string {
  const { userRequest, availableSkills, moduleId, moduleName } = params;

  return `
# AI-Native 应用自生成任务

## 任务上下文
- **模块 ID**: \`${moduleId}\`
- **模块名称**: \`${moduleName}\`
- **用户需求**: ${userRequest}
- **可用 Skill 契约**: ${availableSkills.join(' | ') || '(无 — 自由生成)'}

## 输出要求
生成一个**完整、独立、可运行**的单文件 HTML 应用，满足以下所有约束。

---

${VISUAL_SOVEREIGNTY_SPEC}

---

${BRIDGE_PROTOCOL_SPEC}

---

## 输出格式
请严格按以下 JSON 结构输出（不要包含任何其他文本）：

\`\`\`json
{
  "html": "<完整的 HTML 源码，包含 <!DOCTYPE html>、<head>、Tailwind CDN、<body>，以及所有 JS 逻辑>",
  "permissions": ["db.read", "db.write"],
  "description": "一句话描述该应用的功能"
}
\`\`\`

## 关键检查清单 (生成前自检)
- [ ] HTML 是否包含 \`<!DOCTYPE html>\` 声明？
- [ ] 是否引入了 Tailwind CSS CDN (\`https://cdn.tailwindcss.com\`)？
- [ ] 主容器是否使用了 \`backdrop-blur-md\` 或更强？
- [ ] 边框是否使用了 \`border-white/[0.08]\` 模拟镜面反光？
- [ ] 是否叠加了 \`backdrop-saturate-[1.8]\` 提升折射饱和度？
- [ ] 是否引用了项目 CSS 变量 (\`var(--bg-2)\` 等) 而非硬编码颜色？
- [ ] 是否**绝对没有**使用 \`fetch()\`、\`XMLHttpRequest\` 或试探全局变量？
- [ ] 所有数据请求是否都通过 \`window.natives.db.*\` 网桥代理？
- [ ] 是否监听了 \`token-granted\` 消息以获取会话 Token？
- [ ] 是否在应用就绪后调用了 \`window.natives.lifecycle.ready()\`？

请现在生成完整代码。
`.trim();
}

/**
 * Extract the HTML content from an LLM response.
 * Handles both raw HTML responses and the structured JSON format defined above.
 */
export function extractGeneratedHtml(llmResponse: string): string {
  const trimmed = llmResponse.trim();

  // Try to parse as JSON first (our requested format)
  try {
    const parsed = JSON.parse(trimmed) as { html?: unknown };
    if (parsed.html && typeof parsed.html === 'string') {
      return parsed.html;
    }
  } catch {
    // Not JSON — fall through to other extraction strategies
  }

  // Try to extract from a ```json ... ``` code block
  const jsonBlockMatch = trimmed.match(/```json\s*([\s\S]*?)```/);
  if (jsonBlockMatch && jsonBlockMatch[1]) {
    try {
      const parsed = JSON.parse(jsonBlockMatch[1].trim()) as { html?: unknown };
      if (parsed.html && typeof parsed.html === 'string') {
        return parsed.html;
      }
    } catch {
      // Fall through
    }
  }

  // Try to extract from a ```html ... ``` code block
  const htmlBlockMatch = trimmed.match(/```html\s*([\s\S]*?)```/);
  if (htmlBlockMatch && htmlBlockMatch[1]) {
    return htmlBlockMatch[1].trim();
  }

  // Fall back: return as-is (assume it's raw HTML)
  return trimmed;
}

/**
 * Extract the permissions array from an LLM response.
 * Defaults to ["db.read", "db.write"] if not specified.
 */
export function extractGeneratedPermissions(llmResponse: string): string[] {
  const trimmed = llmResponse.trim();

  try {
    const parsed = JSON.parse(trimmed) as { permissions?: unknown };
    if (Array.isArray(parsed.permissions)) {
      return (parsed.permissions as unknown[]).filter(
        (p): p is string => typeof p === 'string',
      );
    }
  } catch {
    // Fall through
  }

  const jsonBlockMatch = trimmed.match(/```json\s*([\s\S]*?)```/);
  if (jsonBlockMatch && jsonBlockMatch[1]) {
    try {
      const parsed = JSON.parse(jsonBlockMatch[1].trim()) as { permissions?: unknown };
      if (Array.isArray(parsed.permissions)) {
        return (parsed.permissions as unknown[]).filter(
          (p): p is string => typeof p === 'string',
        );
      }
    } catch {
      // Fall through
    }
  }

  // Safe default — minimal permissions
  return ['db.read', 'db.write'];
}
