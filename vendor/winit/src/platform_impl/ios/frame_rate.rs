//! jEngine patch: frames on the display's own clock, at its full rate.
//!
//! A ProMotion iPhone runs an app at 60 Hz unless the app opts out in its
//! plist (`CADisableMinimumFrameDurationOnPhone`, which `jengine-builder`
//! writes) **and** something asks Core Animation for more - normally a
//! `CADisplayLink` with a preferred frame rate range. winit's redraws come
//! from `setNeedsDisplay`, which asks for nothing, so the test lab drew
//! 16.7 ms frames on a 120 Hz phone with the plist key set.
//!
//! [`request`] makes one display link for the life of the app, its range up
//! to the screen's `maximumFramesPerSecond`, and the link **drives the
//! frames**: `Window::request_redraw` marks the window ([`want_redraw`]) and
//! the link's callback hands each marked window its `RedrawRequested`. The
//! link alone was not enough - frames came from `drawRect:`, which UIKit calls
//! in its own update pass, and that pass kept to 60 on a 120 Hz phone with
//! the link asking for 120. Driven from the link they come at the link's
//! rate, and Metal presents on its beat. On a 60 Hz screen it is 60.

use std::cell::Cell;

use objc2::encode::{Encode, Encoding};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_foundation::{MainThreadMarker, NSObject};

use super::app_state::{self, EventWrapper};
use super::window::WinitUIWindow;
use crate::event::{Event, WindowEvent};
use crate::window::WindowId as RootWindowId;

declare_class!(
    struct FrameRateRequest;

    unsafe impl ClassType for FrameRateRequest {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "JEngineFrameRateRequest";
    }

    impl DeclaredClass for FrameRateRequest {}

    unsafe impl FrameRateRequest {
        #[method(tick:)]
        fn tick(&self, _link: *mut AnyObject) {
            let windows = WANTED.with_borrow_mut(std::mem::take);
            if windows.is_empty() {
                return;
            }
            let mtm = MainThreadMarker::from(self);
            app_state::handle_nonuser_events(
                mtm,
                windows.into_iter().map(|window| {
                    EventWrapper::StaticEvent(Event::WindowEvent {
                        window_id: RootWindowId(window.id()),
                        event: WindowEvent::RedrawRequested,
                    })
                }),
            );
        }
    }
);

/// `CAFrameRateRange`.
#[repr(C)]
#[derive(Clone, Copy)]
struct FrameRateRange {
    minimum: f32,
    maximum: f32,
    preferred: f32,
}

unsafe impl Encode for FrameRateRange {
    const ENCODING: Encoding =
        Encoding::Struct("CAFrameRateRange", &[f32::ENCODING, f32::ENCODING, f32::ENCODING]);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    /// `kCFRunLoopCommonModes`, toll-free bridged to `NSRunLoopCommonModes`.
    static kCFRunLoopCommonModes: &'static AnyObject;
}

thread_local! {
    static REQUESTED: Cell<bool> = const { Cell::new(false) };
    /// Whether the link is running, so a redraw can wait for it.
    static RUNNING: Cell<bool> = const { Cell::new(false) };
    /// Windows that asked for a frame since the last tick.
    static WANTED: std::cell::RefCell<Vec<Retained<WinitUIWindow>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Mark `window` for a `RedrawRequested` on the next tick. False before the
/// link runs (a window not yet shown), when the caller draws another way.
pub(crate) fn want_redraw(window: &Retained<WinitUIWindow>) -> bool {
    if !RUNNING.get() {
        return false;
    }
    WANTED.with_borrow_mut(|wanted| {
        if !wanted.iter().any(|w| w == window) {
            wanted.push(window.clone());
        }
    });
    true
}

/// Ask once for the screen's full rate (`max_fps`, the window's screen's
/// `maximumFramesPerSecond`). Safe to call on every window.
pub(crate) fn request(mtm: MainThreadMarker, max_fps: isize) {
    if REQUESTED.replace(true) {
        return;
    }
    let max = max_fps.max(60) as f32;
    let Some(link_class) = AnyClass::get("CADisplayLink") else { return };
    let Some(run_loop_class) = AnyClass::get("NSRunLoop") else { return };
    let target: Retained<FrameRateRequest> = unsafe { msg_send_id![mtm.alloc::<FrameRateRequest>(), init] };
    unsafe {
        let link: *mut AnyObject =
            msg_send![link_class, displayLinkWithTarget: &*target, selector: sel!(tick:)];
        if link.is_null() {
            return;
        }
        let has_range: bool = msg_send![&*link, respondsToSelector: sel!(setPreferredFrameRateRange:)];
        if has_range {
            let range = FrameRateRange { minimum: 60.0_f32.min(max), maximum: max, preferred: max };
            let _: () = msg_send![&*link, setPreferredFrameRateRange: range];
        } else {
            let _: () = msg_send![&*link, setPreferredFramesPerSecond: max as isize];
        }
        let main: *mut AnyObject = msg_send![run_loop_class, mainRunLoop];
        // The real constant: Core Foundation knows the common-modes marker by
        // identity, and a string spelling it registered the link in no mode
        // at all - it never fired.
        let _: () = msg_send![&*link, addToRunLoop: main, forMode: kCFRunLoopCommonModes];
        RUNNING.set(true);
    }
    // The run loop keeps the link, and the link its target; neither is ever
    // invalidated, so the request lasts as long as the app.
    std::mem::forget(target);
}
