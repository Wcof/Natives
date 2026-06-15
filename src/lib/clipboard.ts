/**
 * Clipboard utilities — safe clipboard operations with fallback.
 */

/**
 * Copy text to clipboard. Uses Clipboard API with fallback to execCommand.
 * @returns true if successful
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Clipboard API not available, try fallback
  }

  // Fallback: execCommand('copy')
  try {
    const textarea = document.createElement('textarea');
    textarea.value = text;
    textarea.style.cssText = 'position:fixed;left:-9999px;top:-9999px;opacity:0';
    document.body.appendChild(textarea);
    textarea.select();
    const ok = document.execCommand('copy');
    document.body.removeChild(textarea);
    return ok;
  } catch {
    return false;
  }
}

/**
 * Read text from clipboard.
 * @returns clipboard text or null if unavailable
 */
export async function readFromClipboard(): Promise<string | null> {
  try {
    if (navigator.clipboard?.readText) {
      return await navigator.clipboard.readText();
    }
  } catch {
    // Clipboard API not available
  }
  return null;
}
