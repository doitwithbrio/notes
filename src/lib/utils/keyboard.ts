function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return Boolean(target.closest('input, textarea, select, [contenteditable="true"], [contenteditable=""]'));
}

export function shouldIgnoreGlobalShortcut(event: KeyboardEvent): boolean {
  if (event.defaultPrevented) return true;
  if (event.isComposing) return true;
  if (event.getModifierState?.('AltGraph')) return true;
  return isEditableTarget(event.target);
}
