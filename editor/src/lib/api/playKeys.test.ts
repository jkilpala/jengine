import { describe, expect, it } from "vitest";
import { routeKey, type KeyLike, type PlayKeyState } from "./playKeys";

function key(code: string, mods: Partial<KeyLike> = {}, k?: string): KeyLike {
  return {
    code,
    key: k ?? (code.startsWith("Key") ? code.slice(3).toLowerCase() : code),
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...mods,
  };
}

const running = (view: "scene" | "game"): PlayKeyState => ({
  playing: true,
  paused: false,
  view,
  editable: false,
  textSelected: false,
});

const pause = key("KeyP", { ctrlKey: true, shiftKey: true });
const step = key("KeyP", { ctrlKey: true, altKey: true });
const stop = key("F5", { shiftKey: true });

describe("routeKey", () => {
  it("gives the editor every key outside play and while paused", () => {
    const idle = { ...running("game"), playing: false };
    const paused = { ...running("game"), paused: true };
    for (const k of [key("KeyW"), key("KeyZ", { ctrlKey: true }), key("Escape")]) {
      expect(routeKey(k, idle)).toBe("editor");
      expect(routeKey(k, paused)).toBe("editor");
    }
  });

  it("lets only the play controls through in the Game view", () => {
    const s = running("game");
    for (const k of [pause, step, stop]) expect(routeKey(k, s)).toBe("editor");
    for (const k of [
      key("KeyW"),
      key("Escape"),
      key("F3"),
      key("F12"),
      key("F5"), // reload - the game's, not a way to lose the session
      key("KeyS", { ctrlKey: true }),
      key("KeyZ", { ctrlKey: true }),
      key("KeyA", { ctrlKey: true, shiftKey: true }),
      key("KeyP", { ctrlKey: true }),
    ]) {
      expect(routeKey(k, s)).toBe("game");
    }
  });

  it("keeps the Scene view's tools and navigation, and nothing that edits", () => {
    const s = running("scene");
    for (const k of [
      pause,
      stop,
      key("KeyW"),
      key("KeyE"),
      key("KeyF"),
      key("KeyC"),
      key("Escape"),
      key("F3"),
      key("KeyA", { ctrlKey: true, shiftKey: true }),
      key("KeyP", { ctrlKey: true }),
    ]) {
      expect(routeKey(k, s)).toBe("editor");
    }
    for (const k of [
      key("KeyA"),
      key("Space", {}, " "),
      key("ArrowUp", {}, "ArrowUp"),
      key("Delete"),
      key("KeyW", { shiftKey: true }),
      key("KeyZ", { ctrlKey: true }),
      key("KeyV", { ctrlKey: true }),
      key("KeyS", { ctrlKey: true }),
    ]) {
      expect(routeKey(k, s)).toBe("game");
    }
  });

  it("gives a text field its keystrokes in either view", () => {
    for (const view of ["scene", "game"] as const) {
      const s = { ...running(view), editable: true };
      expect(routeKey(key("KeyW"), s)).toBe("editor");
      expect(routeKey(key("KeyS", { ctrlKey: true }), s)).toBe("editor");
    }
  });

  it("treats Ctrl+C over selected text as a copy", () => {
    const copy = key("KeyC", { ctrlKey: true });
    expect(routeKey(copy, running("game"))).toBe("game");
    expect(routeKey(copy, { ...running("game"), textSelected: true })).toBe("editor");
  });

  it("does not mistake plain F5 for stop", () => {
    expect(routeKey(key("F5"), running("scene"))).toBe("game");
  });
});
