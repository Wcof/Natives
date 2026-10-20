// Provider identity accents; all surfaces and state colors use product tokens.
(function(){
  // Authentic Brand Colors from Token Monitor
  const CLIENT_COLORS = {
    claude: '#cc7c5e',
    codex: '#49a3b0',
    opencode: '#3b82f6',
    hermes: '#d4af37',
    openclaw: '#ff4d4d',
    cursor: '#00b4d8',
    antigravity: '#4285f4',
    cline: '#323b43',
    amp: '#f34e3f',
    droid: '#10b981',
    kimi: '#6366f1',
    qwen: '#615ced',
    grok: '#f43f5e',
    copilot: '#10a37f',
    pi: '#8b5cf6',
    zed: '#4173e7',
    kilo: '#f8f676',
    commandcode: '#8c4edd',
    zcode: '#ec4899',
    kiro: '#9046ff',
    codebuddy: '#6c4dff',
    workbuddy: '#0dc8a5',
    proma: '#64748b',
    qodercn: '#2adb5c',
    reasonix: '#4d6bfe',
    dsh: '#4d6bfe',
    cherrystudio: '#ea5e5d',
    lmstudio: '#6c5ce7',
    unsloth: '#40b85a',
    openrouter: '#6566f1',
    gemini: '#4285f4',
    deepseek: '#4d6bfe',
    minimax: '#f23f5d',
    volcengine: '#006eff',
    alibaba: '#615ced',
    default: 'var(--natives-accent)'
  };

  function getClientColor(id) {
    return CLIENT_COLORS[id] || CLIENT_COLORS.default;
  }

  window.TokenClientColor=getClientColor;
})();
