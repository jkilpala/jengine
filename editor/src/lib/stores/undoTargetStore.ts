/**
 * Who Ctrl+Z belongs to. The app's key handler undoes the scene (or the
 * prefab editor) - but an editor open over it keeps a history of its own,
 * and since the app's window listener runs before an editor's, an editor
 * calling `preventDefault` could not stop the scene being undone too: one
 * keypress undid in both. An editor with its own history claims undo while
 * it is mounted (`claimUndo`), and the app's handler asks `undoTarget()`
 * first. The most recently mounted claim wins, so an editor opened over
 * another takes the keys and gives them back when it closes.
 */
export interface UndoTarget {
  undo(): void;
  redo(): void;
  /** Leave Ctrl+Z in a text field to the field's own undo. */
  ignoreTyping?: boolean;
}

const claims: UndoTarget[] = [];

/** Take Ctrl+Z / Ctrl+Y for `target` until the returned function is called
 *  (call it from the component's teardown). */
export function claimUndo(target: UndoTarget): () => void {
  claims.push(target);
  return () => {
    const i = claims.lastIndexOf(target);
    if (i >= 0) claims.splice(i, 1);
  };
}

/** The editor that should get undo now, or null for the scene. */
export function undoTarget(): UndoTarget | null {
  return claims.length > 0 ? claims[claims.length - 1] : null;
}
