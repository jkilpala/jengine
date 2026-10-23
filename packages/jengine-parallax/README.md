# jengine-parallax

Parallax scrolling: background layers that move slower than the camera and
foreground layers that move faster, so a flat scene reads as deep.

This is a **built-in** package: the native runtime, the web runtime and the
editor viewport are compiled with it. Unlike physics, audio and particles it
has no hard-coded path in any runtime - its `parallax_scroll` system runs
only because each host runs package systems, which is what made it the
package that proved the package system works (scene 59 in the Engine Test
Lab).

## What it registers

| Component | Is |
|---|---|
| `parallax_layer` | `scroll_factor_x` / `_y` (0 stays put on screen, 1 moves with the world, above 1 is foreground), `repeat_x` / `_y` with `repeat_width` / `_height` to wrap a strip, `offset_x` / `_y`, `auto_scroll_x` / `_y` in units per second, and `z_order_override`. |

`parallax_layer` is a **package** component: a scene that uses it must be a
v2 scene (`"version": 2`).

The layer's transform is rewritten every frame at `PreRender` from the active
camera, so set the layer up through the component, not by moving it.

## From scripts

| Call | Does |
|---|---|
| `Parallax.set(e, x, y)` | Set the scroll factors. |
| `Parallax.set_auto_scroll(e, x, y)` | Set the auto-scroll speed. |

Both work in every runtime. The older flat globals `set_parallax` and
`set_parallax_auto_scroll` are installed by the native runtime only (they are
`ScriptApiRegistry` handlers, which the editor and web do not run) - use the
`Parallax` namespace.

See the [Parallax guide](/guides/parallax/).
