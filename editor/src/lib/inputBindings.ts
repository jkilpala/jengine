/**
 * Input-action bindings as the editor offers and captures them. A binding is
 * a name (`input::gamepad`): a key (`"Space"`, `"W"`, `"Up"`), a pad button
 * (`"Pad0_A"`), or a stick or trigger with a direction
 * (`"Pad0_LeftStickX+"`). The names come from `inputNames.json`, which a
 * Rust test checks against the engine's lists.
 */
import names from "./inputNames.json";

export const KEY_NAMES: string[] = names.keys;
export const PAD_BUTTONS: string[] = names.pad_buttons;
export const PAD_AXES: string[] = names.pad_axes;

/** Every binding name for pads `0..pads`: keys, then each pad's buttons and
 *  each axis both ways. */
export function bindingChoices(pads = 1): string[] {
  const out = [...KEY_NAMES];
  for (let p = 0; p < pads; p++) {
    for (const b of PAD_BUTTONS) out.push(`Pad${p}_${b}`);
    for (const a of PAD_AXES) out.push(`Pad${p}_${a}+`, `Pad${p}_${a}-`);
  }
  return out;
}

/** Whether `name` is a binding the engine resolves. */
export function isBinding(name: string): boolean {
  return bindingChoices(4).includes(name);
}

/** A browser `KeyboardEvent.code` as the key's binding name (`"KeyW"` ->
 *  `"W"`, `"ArrowUp"` -> `"Up"`, `"Digit1"` -> `"1"`), or null. */
export function keyBinding(code: string): string | null {
  const name = code.replace(/^Key(?=[A-Z]$)/, "").replace(/^Digit(?=\d$)/, "").replace(/^Arrow/, "");
  return KEY_NAMES.includes(name) ? name : null;
}

/** One pad's state, as the Gamepad API reports it. */
export interface PadState {
  /** `buttons[i].value`, 0 to 1. */
  buttons: number[];
  /** `axes[i]`, -1 to 1, stick Y positive **down** as browsers report it. */
  axes: number[];
}

/** What a pad did between two looks, as a binding - the first button that
 *  went down, else the first axis that went past `threshold` - or null.
 *  Stick Y is turned to the engine's positive-up (`gamepad::stick_y_up`),
 *  so pushing a stick up binds `LeftStickY+`. */
export function padBinding(prev: PadState, now: PadState, pad = 0, threshold = 0.5): string | null {
  for (let i = 0; i < now.buttons.length && i < PAD_BUTTONS.length; i++) {
    if ((now.buttons[i] ?? 0) > 0.5 && (prev.buttons[i] ?? 0) <= 0.5) return `Pad${pad}_${PAD_BUTTONS[i]}`;
  }
  for (let i = 0; i < now.axes.length && i < PAD_AXES.length; i++) {
    const yAxis = i === 1 || i === 3;
    const v = (now.axes[i] ?? 0) * (yAxis ? -1 : 1);
    const was = (prev.axes[i] ?? 0) * (yAxis ? -1 : 1);
    if (Math.abs(v) > threshold && Math.abs(was) <= threshold) return `Pad${pad}_${PAD_AXES[i]}${v > 0 ? "+" : "-"}`;
  }
  return null;
}
