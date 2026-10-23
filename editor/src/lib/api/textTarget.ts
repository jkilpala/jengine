/**
 * Whether a key event landed in something the user is typing into.
 *
 * A global shortcut must not fire while a name is being edited: F2 renaming
 * an asset is helpful, F2 doing it again mid-rename is not. Kept here rather
 * than in one component because every window-level handler needs the same
 * answer.
 */
export function isTextEditingTarget(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  const tag = t.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return true;
  if (t.isContentEditable) return true;
  // CodeMirror keeps its own editable surface.
  if (t.closest(".cm-editor")) return true;
  return false;
}
