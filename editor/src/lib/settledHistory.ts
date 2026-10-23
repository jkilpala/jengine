/**
 * Undo for an editor that keeps its document in component state and changes
 * it from many places - the shader graph, the atlas and spritesheet editors,
 * project settings. Rather than recording before each of dozens of mutation
 * sites, the history **watches** the document (`observe`, from an `$effect`)
 * and, when it has changed and then stayed still for `delay` milliseconds,
 * keeps the state it changed from as one undo step. A drag, or a word typed
 * into a field, is therefore one step, and a new mutation site needs no
 * wiring at all.
 *
 * Snapshots are JSON, so the document must be plain data.
 */
export class SettledHistory<T> {
  private settled: string;
  private undoStack: string[] = [];
  private redoStack: string[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** The text `write` was last handed - its echo through `observe` is not
   *  a new edit. */
  private applied: string | null = null;

  constructor(
    private read: () => T,
    private write: (value: T) => void,
    private delay = 400,
    private limit = 100,
    /** Told whenever undo / redo availability may have changed. */
    private onChange: () => void = () => {},
  ) {
    this.settled = JSON.stringify(read());
  }

  /** Look at the document: call whenever it may have changed. */
  observe(): void {
    const now = JSON.stringify(this.read());
    if (this.applied !== null && now === this.applied) {
      // The echo of an undo or redo: the new settled state, not an edit.
      this.applied = null;
      this.settled = now;
      this.cancel();
      return;
    }
    if (now === this.settled) {
      this.cancel();
      return;
    }
    this.cancel();
    this.timer = setTimeout(() => this.settle(), this.delay);
  }

  /** Make a pending change a step now (before an undo, a save, a close). */
  flush(): void {
    if (this.timer !== null) this.settle();
  }

  private cancel() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  private settle() {
    this.cancel();
    const now = JSON.stringify(this.read());
    if (now === this.settled) return;
    this.undoStack.push(this.settled);
    if (this.undoStack.length > this.limit) this.undoStack.shift();
    this.redoStack = [];
    this.settled = now;
    this.onChange();
  }

  get canUndo(): boolean {
    return this.undoStack.length > 0 || this.timer !== null;
  }

  get canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  undo(): boolean {
    this.flush();
    const prev = this.undoStack.pop();
    if (prev === undefined) return false;
    this.redoStack.push(this.settled);
    this.apply(prev);
    return true;
  }

  redo(): boolean {
    this.flush();
    const next = this.redoStack.pop();
    if (next === undefined) return false;
    this.undoStack.push(this.settled);
    this.apply(next);
    return true;
  }

  private apply(text: string) {
    this.settled = text;
    this.applied = text;
    this.write(JSON.parse(text) as T);
    this.onChange();
  }

  /** Forget the history: a new document was loaded (or saved as the
   *  baseline). */
  reset(): void {
    this.cancel();
    this.undoStack = [];
    this.redoStack = [];
    this.applied = null;
    this.settled = JSON.stringify(this.read());
    this.onChange();
  }
}

/** Whether a key event is undo (`"undo"`), redo (`"redo"`) or neither -
 *  Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z and Ctrl+Y. Typing in a text field is left
 *  to the field's own undo. */
export function undoKey(e: KeyboardEvent): "undo" | "redo" | null {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return null;
  const t = e.target as HTMLElement | null;
  if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) {
    const type = (t as HTMLInputElement).type;
    if (t.tagName === "TEXTAREA" || t.isContentEditable || ["text", "search", "number", ""].includes(type)) return null;
  }
  const k = e.key.toLowerCase();
  if (k === "z") return e.shiftKey ? "redo" : "undo";
  if (k === "y") return "redo";
  return null;
}
