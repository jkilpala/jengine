# winit 0.30.13, patched for jEngine

This is winit 0.30.13 from crates.io, with its examples, tests and docs left
out, and three changes: an **input hook on Android**, and on iOS the
**UIScene lifecycle** and a **frame loop that keeps going** (below).

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

And a third, also iOS: **the frame loop stopped after one frame**.
`RedrawRequested` is delivered from inside `drawRect:`, and a game asks for
its next frame from that handler - but a `setNeedsDisplay` made inside
`drawRect:` is cleared by UIKit when it returns, so the request was lost and
the app drew once and froze (taps and keys arrived; nothing ran to read
them). `Window::request_redraw` in `src/platform_impl/ios/window.rs` now
sends `setNeedsDisplay` on the next turn of the run loop
(`performSelector:withObject:afterDelay:`). winit's own way round it, the
`gl_or_metal_backed` queue, never fires: it asks whether the *view* is a
layer class.

Not patched, and worth knowing: winit 0.30 on iOS reports **no hardware
keyboard keys** - only text typed into the soft keyboard, and Backspace.

Every other platform is untouched.

The root `Cargo.toml` and both crates `jengine-builder` generates (Android,
and the desktop / iOS runtime) point `[patch.crates-io] winit` here - a
generated crate that leaves it out builds with stock winit and, on iOS,
shows a black launch screen forever. To move to a newer winit, re-copy its
`src/`, `build.rs`, `Cargo.toml` (without the `[[example]]` / `[[test]]`
targets), `LICENSE` and `README.md`, and re-apply the marked edits - or drop
this directory once winit forwards controllers and adopts UIScene itself.
