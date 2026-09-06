

export function createSpaceResetModal({ $, onResetWorkspace }) {
  let selectedTemplate = 'classic';
  let resetContext = null;

  const modal = $('ws-reset-modal');
  const options = modal?.querySelectorAll?.('[data-reset-template]') || [];

  function select(template) {
    selectedTemplate = template;
    options.forEach((option) => {
      option.setAttribute('aria-pressed', String(option.dataset.resetTemplate === template));
    });
  }

  options.forEach((option) => {
    option.onclick = () => select(option.dataset.resetTemplate);
  });

  function open(context) {
    if (!context || !modal) return;
    resetContext = context;
    modal.returnValue = '';
    select('classic');
    if (!modal.open && typeof modal.showModal === 'function') modal.showModal();
    options[0]?.focus?.();
    modal.onclose = () => {
      if (modal.returnValue !== 'default' || !resetContext) return;
      onResetWorkspace(resetContext, selectedTemplate);
    };
  }

  return { open };
}
