# winit 0.30.13, patched for jEngine

This is winit 0.30.13 from crates.io, with its examples, tests and docs left
out, and four changes: an **input hook on Android**, and on iOS the
**UIScene lifecycle**, **frames on the display link** and **hardware keys**
(below).

winit has no gamepad support and does not look at an Android input event's
source, so a thumbstick's joystick `MotionEvent` became a *touch* at the
stick's own `-1..1` coordinates - a press in the top-left corner of the screen
- and a controller's buttons became unidentified keys. There is no way to
reach the events from outside: winit drains `android-activity`'s input queue
itself. So:

- `src/platform/android.rs` gains `InputHook`, `set_input_hook` and the static
  that holds it (at the bottom, marked "jEngine patch").
- `src/platform_impl/android/mod.rs` calls the hook first thing in
  `handle_input_event`; an event it claims is not translated by winit.

`jengine-runtime`'s `gamepad_android` installs the hook and claims game
controllers.

And a second: **the UIScene lifecycle on iOS** (Oct 2026). winit 0.30 drives
UIKit without scenes, and iOS 27 traps an app that has not adopted them
(SIGTRAP in `_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`
as the first scene is created). The app's `Info.plist` declares a
`UIApplicationSceneManifest` (`jengine_builder::ios`), which makes it
scene-based - and in a scene-based app a window in no `UIWindowScene` is
never shown, leaving the launch screen up. So:

- `src/platform_impl/ios/scene.rs` (new): `show` puts a window in the app's
  window scene and makes it key, or holds it until
  `UISceneWillConnectNotification` brings one. No scene delegate class.
- `src/platform_impl/ios/app_state.rs`: the three `makeKeyAndVisible` calls
  go through `scene::show` (marked "jEngine patch").
- `src/platform_impl/ios/mod.rs` declares the module; `Cargo.toml` adds the
  `UIScene` and `UIWindowScene` features of `objc2-ui-kit`.

And a third, also iOS: **frames come from a display link**
(`src/platform_impl/ios/frame_rate.rs`). winit delivers `RedrawRequested`
from `drawRect:`, and that went wrong twice. A `setNeedsDisplay` made
*inside* `drawRect:` - where every game asks for its next frame - is cleared
by UIKit when it returns, so the app drew one frame and froze (taps and keys
arrived; nothing ran to read them). And once that was worked round, frames
still came from UIKit's own display pass, which kept to 60 Hz on a 120 Hz
ProMotion iPhone. So one `CADisplayLink`, made when the first window is
shown, asks for the screen's `maximumFramesPerSecond` and drives the frames:
`Window::request_redraw` marks the window (`frame_rate::want_redraw`) and the
link's callback hands it `RedrawRequested`. Before the link runs, a redraw
falls back to `setNeedsDisplay` on the next turn of the run loop
(`performSelector:withObject:afterDelay:`). The link is added for
`kCFRunLoopCommonModes` **by the real constant** - a string spelling it
registered the link in no mode and it never fired. The plist key
`CADisableMinimumFrameDurationOnPhone`, which `jengine-builder` writes, is
the other half of 120 Hz. (winit's own `gl_or_metal_backed` queue never
fires: it asks whether the *view* is a layer class.)

And a fourth, also iOS: **hardware keyboard keys**
(`src/platform_impl/ios/keyboard.rs`). winit 0.30 on iOS reports only text
typed into the soft keyboard, and Backspace. `WinitUIWindow` now takes
`pressesBegan` / `pressesEnded` / `pressesCancelled` - it ends the responder
chain, so it hears a press whether or not the view is first responder - and
turns each `UIKey`'s HID usage into a `KeyCode`, with no text (text still
comes from `insertText:`, so a focused field gets nothing twice).

Every other platform is untouched.

The root `Cargo.toml` and both crates `jengine-builder` generates (Android,
and the desktop / iOS runtime) point `[patch.crates-io] winit` here - a
generated crate that leaves it out builds with stock winit and, on iOS,
shows a black launch screen forever. To move to a newer winit, re-copy its
`src/`, `build.rs`, `Cargo.toml` (without the `[[example]]` / `[[test]]`
targets), `LICENSE` and `README.md`, and re-apply the marked edits - or drop
this directory once winit forwards controllers and adopts UIScene itself.
