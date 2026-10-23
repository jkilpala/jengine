import { describe, expect, it } from "vitest";
import { bindingChoices, isBinding, keyBinding, padBinding } from "./inputBindings";

describe("binding names", () => {
  it("offers keys, then pad buttons and both directions of each axis", () => {
    const all = bindingChoices();
    expect(all).toContain("Space");
    expect(all).toContain("Pad0_A");
    expect(all).toContain("Pad0_LeftStickY+");
    expect(all).toContain("Pad0_RightStickX-");
    expect(all).not.toContain("Pad1_A");
    expect(isBinding("Pad3_DPadLeft")).toBe(true);
    expect(isBinding("Pad0_LeftStickX")).toBe(false);
  });

  it("turns a browser key code into the key's name", () => {
    expect(keyBinding("KeyW")).toBe("W");
    expect(keyBinding("Digit1")).toBe("1");
    expect(keyBinding("ArrowUp")).toBe("Up");
    expect(keyBinding("Space")).toBe("Space");
    expect(keyBinding("ShiftLeft")).toBe("ShiftLeft");
    expect(keyBinding("Numpad7")).toBe("Numpad7");
    expect(keyBinding("MediaPlayPause")).toBeNull();
  });
});

describe("press to bind on a pad", () => {
  const rest = { buttons: Array(17).fill(0), axes: [0, 0, 0, 0] };
  it("takes the button that went down", () => {
    const now = { ...rest, buttons: rest.buttons.map((_, i) => (i === 0 ? 1 : 0)) };
    expect(padBinding(rest, now)).toBe("Pad0_A");
    // Held from before is not a new press.
    expect(padBinding(now, now)).toBeNull();
  });

  it("takes a stick past the threshold, with up as plus", () => {
    // Browsers report up as negative Y.
    expect(padBinding(rest, { ...rest, axes: [0, -0.9, 0, 0] }, 0)).toBe("Pad0_LeftStickY+");
    expect(padBinding(rest, { ...rest, axes: [0.8, 0, 0, 0] }, 1)).toBe("Pad1_LeftStickX+");
    expect(padBinding(rest, { ...rest, axes: [0, 0, -0.7, 0] })).toBe("Pad0_RightStickX-");
    expect(padBinding(rest, { ...rest, axes: [0.3, 0, 0, 0] })).toBeNull();
  });
});
