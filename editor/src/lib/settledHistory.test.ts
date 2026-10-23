import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettledHistory, undoKey } from "./settledHistory";

describe("settled history", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const make = () => {
    let doc = { n: 0, name: "a" };
    const h = new SettledHistory(
      () => doc,
      (v) => {
        doc = v;
        h.observe(); // as the component's $effect would
      },
      400,
    );
    return { h, get: () => doc, set: (d: typeof doc) => ((doc = d), h.observe()) };
  };

  it("makes a burst of changes one step once it settles", () => {
    const { h, get, set } = make();
    set({ n: 1, name: "a" });
    vi.advanceTimersByTime(100);
    set({ n: 2, name: "a" });
    vi.advanceTimersByTime(100);
    set({ n: 3, name: "a" });
    vi.advanceTimersByTime(500);
    expect(h.undo()).toBe(true);
    expect(get()).toEqual({ n: 0, name: "a" });
    expect(h.undo()).toBe(false);
    expect(h.redo()).toBe(true);
    expect(get()).toEqual({ n: 3, name: "a" });
  });

  it("separates changes that settled apart, and undoes a pending one at once", () => {
    const { h, get, set } = make();
    set({ n: 1, name: "a" });
    vi.advanceTimersByTime(500);
    set({ n: 1, name: "b" });
    // Not settled yet: undo still takes it back.
    expect(h.canUndo).toBe(true);
    h.undo();
    expect(get()).toEqual({ n: 1, name: "a" });
    h.undo();
    expect(get()).toEqual({ n: 0, name: "a" });
  });

  it("does not record its own undo as an edit, and a new edit clears redo", () => {
    const { h, get, set } = make();
    set({ n: 1, name: "a" });
    vi.advanceTimersByTime(500);
    h.undo();
    vi.advanceTimersByTime(500);
    expect(h.canRedo).toBe(true);
    set({ n: 5, name: "a" });
    vi.advanceTimersByTime(500);
    expect(h.canRedo).toBe(false);
    h.undo();
    expect(get()).toEqual({ n: 0, name: "a" });
  });

  it("a change undone by hand back to the settled state is no step", () => {
    const { h, set } = make();
    set({ n: 1, name: "a" });
    set({ n: 0, name: "a" });
    vi.advanceTimersByTime(500);
    expect(h.canUndo).toBe(false);
  });

  it("forgets on reset", () => {
    const { h, set } = make();
    set({ n: 1, name: "a" });
    vi.advanceTimersByTime(500);
    h.reset();
    expect(h.canUndo).toBe(false);
    expect(h.undo()).toBe(false);
  });
});

describe("undo keys", () => {
  const key = (k: string, mods: Partial<KeyboardEvent> = {}, target: unknown = null) =>
    undoKey({ key: k, ctrlKey: true, metaKey: false, shiftKey: false, altKey: false, target, ...mods } as unknown as KeyboardEvent);
  it("reads Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y, and leaves text fields their own", () => {
    expect(key("z")).toBe("undo");
    expect(key("Z", { shiftKey: true })).toBe("redo");
    expect(key("y")).toBe("redo");
    expect(key("z", { ctrlKey: false })).toBeNull();
    expect(key("z", {}, { tagName: "INPUT", type: "text" })).toBeNull();
    expect(key("z", {}, { tagName: "INPUT", type: "checkbox" })).toBe("undo");
    expect(key("z", {}, { tagName: "TEXTAREA" })).toBeNull();
  });
});
