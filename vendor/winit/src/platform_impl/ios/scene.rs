//! jEngine patch: the UIScene lifecycle.
//!
//! winit 0.30 drives UIKit the pre-iOS-13 way - one `UIWindow` per screen,
//! no scenes - and iOS 27 refuses that outright: UIKit traps
//! (`_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`,
//! SIGTRAP) as the first scene is created, unless the app's `Info.plist`
//! declares a `UIApplicationSceneManifest`. Declaring one makes the app
//! scene-based, and in a scene-based app a window that belongs to no
//! `UIWindowScene` is never shown.
//!
//! So every window winit shows goes through [`show`]: it joins the app's
//! window scene and becomes key. winit makes its first window while
//! launching, before UIKit has connected any scene, so such a window waits
//! here until `UISceneWillConnectNotification` delivers one. Observing the
//! notification needs no scene delegate class, which keeps the app's
//! delegate the user's, as winit wants.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{msg_send, ClassType, Message};
use objc2_foundation::{MainThreadMarker, NSNotificationCenter};
use objc2_ui_kit::{UIApplication, UICoordinateSpace, UISceneWillConnectNotification, UIWindowScene};

use super::notification_center::create_observer;
use super::window::WinitUIWindow;

thread_local! {
    /// Windows asked to show before any window scene was connected.
    static WAITING: RefCell<Vec<Retained<WinitUIWindow>>> = const { RefCell::new(Vec::new()) };
    static OBSERVING: Cell<bool> = const { Cell::new(false) };
}

/// `makeKeyAndVisible`, in the app's window scene - now, or once one connects.
pub(crate) fn show(mtm: MainThreadMarker, window: &Retained<WinitUIWindow>) {
    if let Some(scene) = connected_window_scene(mtm) {
        attach(window, &scene);
        return;
    }
    WAITING.with_borrow_mut(|waiting| waiting.push(window.clone()));
    observe_connections();
}

fn attach(window: &WinitUIWindow, scene: &UIWindowScene) {
    unsafe {
        window.setWindowScene(Some(scene));
        // The scene's bounds, not the screen's the window was made with: the
        // two differ when the scene does not fill the screen (an iPad's split
        // view, Stage Manager).
        window.setFrame(scene.coordinateSpace().bounds());
    }
    window.makeKeyAndVisible();
    // jEngine patch: a ProMotion screen runs at 60 Hz unless asked.
    super::frame_rate::request(MainThreadMarker::from(window), window.screen().maximumFramesPerSecond());
}

fn as_window_scene<T: Message>(object: Retained<T>) -> Option<Retained<UIWindowScene>> {
    let is_window_scene: bool = unsafe { msg_send![&*object, isKindOfClass: UIWindowScene::class()] };
    // SAFETY: checked to be a `UIWindowScene` just above.
    is_window_scene.then(|| unsafe { Retained::cast(object) })
}

fn connected_window_scene(mtm: MainThreadMarker) -> Option<Retained<UIWindowScene>> {
    let app = UIApplication::sharedApplication(mtm);
    let scenes = unsafe { app.connectedScenes() };
    let scenes = unsafe { scenes.allObjects() };
    let found = scenes.iter().find_map(|scene| as_window_scene(scene.retain()));
    found
}

fn observe_connections() {
    if OBSERVING.replace(true) {
        return;
    }
    let center = unsafe { NSNotificationCenter::defaultCenter() };
    let observer = create_observer(&center, unsafe { UISceneWillConnectNotification }, |note| {
        let Some(scene) = (unsafe { note.object() }).and_then(as_window_scene) else { return };
        for window in WAITING.with_borrow_mut(std::mem::take) {
            attach(&window, &scene);
        }
    });
    // Observed for the life of the app, as winit's own observers are.
    std::mem::forget(observer);
}
