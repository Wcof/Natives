// ADR-0038: credentials are entered only in the trusted owner document.
export function createModuleSecretBroker({ context, t }) {
  let dialog = null;
  let cancel = null;
  function dispose() { cancel?.(); }
  async function handle(event) {
    const m = event.data, c = context();
    if (m?.type !== 'natives:secret') return false;
    if (event.source !== c.frame?.contentWindow || m.generation !== c.generation ||
        m.challenge !== c.challenge || !/^[a-zA-Z0-9-]{1,128}$/.test(m.requestId || '')) return true;
    const reply = (result, error) => {
      const now = context();
      if (now.frame === c.frame && now.generation === c.generation && now.challenge === c.challenge) {
        c.frame.contentWindow.postMessage({ type: 'natives:reply', requestId: m.requestId,
          generation: c.generation, challenge: c.challenge, result, error }, '*');
      }
    };
    try {
      if (!c.capabilities.includes('platform.secret') || !c.host || dialog ||
          !['edit', 'delete'].includes(m.op) || !/^[a-z0-9][a-z0-9:._-]{0,63}$/.test(m.accountId || '') ||
          Object.keys(m).some(k => !['type','requestId','generation','challenge','op','accountId'].includes(k))) {
        throw Error(t('moduleSecretUnavailable'));
      }
      const focus = document.activeElement;
      const node = dialog = document.createElement('dialog');
      node.className = 'module-secret-dialog';
      const title = document.createElement('h2');
      title.textContent = t(m.op === 'delete' ? 'moduleSecretDelete' : 'moduleSecretTitle');
      title.id = 'module-secret-title';
      node.setAttribute('aria-labelledby', title.id);
      const ref = document.createElement('p'); ref.textContent = m.accountId;
      const form = document.createElement('form');
      const input = document.createElement('input'); input.type = 'password';
      input.autocomplete = 'new-password'; input.required = true; input.maxLength = 8192;
      input.setAttribute('aria-label', t('moduleSecretValue'));
      const warning = document.createElement('p'); warning.textContent = t('moduleSecretPrivacy');
      const error = document.createElement('p'); error.setAttribute('role', 'status');
      const no = document.createElement('button'); no.type = 'button'; no.textContent = t('cancel');
      const yes = document.createElement('button'); yes.type = 'submit';
      yes.textContent = t(m.op === 'delete' ? 'moduleSecretDelete' : 'save');
      if (m.op === 'edit') form.append(input);
      form.append(warning, error, no, yes); node.append(title, ref, form); document.body.append(node);
      await new Promise(resolve => {
        let finished = false;
        const onHidden = () => { if (document.hidden) cancel?.(); };
        const onPageHide = () => cancel?.();
        const finish = result => {
          if (finished) return; finished = true;
          document.removeEventListener('visibilitychange', onHidden);
          window.removeEventListener('pagehide', onPageHide);
          input.value = ''; node.close(); node.remove(); dialog = null; cancel = null;
          if (focus?.isConnected) focus.focus(); reply(result); resolve();
        };
        cancel = () => finish({ cancelled: true });
        document.addEventListener('visibilitychange', onHidden);
        window.addEventListener('pagehide', onPageHide, { once: true });
        no.onclick = cancel; node.oncancel = e => { e.preventDefault(); cancel?.(); };
        form.onsubmit = async e => {
          e.preventDefault(); yes.disabled = true;
          try {
            const now = context();
            if (now.frame !== c.frame || now.generation !== c.generation || now.challenge !== c.challenge) {
              finish({ cancelled: true }); return;
            }
            const params = { instanceId: c.instanceId, generation: c.generation, accountId: m.accountId,
              op: m.op === 'edit' ? 'set' : 'delete' };
            if (m.op === 'edit') params.value = input.value; else params.confirm = true;
            await c.host.call('app:secret', params);
            finish({ saved: true });
          } catch { error.textContent = t('moduleSecretUnavailable'); }
          finally { yes.disabled = false; }
        };
        node.showModal(); (m.op === 'edit' ? input : no).focus();
      });
    } catch (error) { reply(null, error.message); }
    return true;
  }
  return { handle, dispose };
}
