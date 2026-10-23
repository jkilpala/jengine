import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { dismissToast, MAX_TOASTS, toast, toasts } from "./toastStore";

beforeEach(() => {
  vi.useFakeTimers();
  toasts.set([]);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("toasts", () => {
  it("go by themselves, an error later than a success", () => {
    toast({ kind: "ok", title: "Build finished" });
    toast({ kind: "err", title: "Script error", detail: "hud.lua:77" });
    vi.advanceTimersByTime(5001);
    expect(get(toasts).map((t) => t.title)).toEqual(["Script error"]);
    vi.advanceTimersByTime(3000);
    expect(get(toasts)).toEqual([]);
  });

  it("the same toast again is refreshed, not stacked", () => {
    const a = toast({ kind: "err", title: "Script error", detail: "hud.lua:77" });
    vi.advanceTimersByTime(6000);
    const b = toast({ kind: "err", title: "Script error", detail: "hud.lua:77" });
    expect(b).toBe(a);
    expect(get(toasts)).toHaveLength(1);
    vi.advanceTimersByTime(6000);
    expect(get(toasts)).toHaveLength(1); // its life restarted
  });

  it("keeps the newest few", () => {
    for (let i = 0; i < MAX_TOASTS + 2; i++) toast({ kind: "info", title: `t${i}` });
    const titles = get(toasts).map((t) => t.title);
    expect(titles).toHaveLength(MAX_TOASTS);
    expect(titles[titles.length - 1]).toBe(`t${MAX_TOASTS + 1}`);
  });

  it("can be dismissed", () => {
    const id = toast({ kind: "info", title: "x" });
    dismissToast(id);
    expect(get(toasts)).toEqual([]);
  });
});
