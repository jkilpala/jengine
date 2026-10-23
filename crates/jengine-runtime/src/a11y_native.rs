//! The UI handed to the platform's screen reader (`jengine_core::ui::
//! accessibility`) through AccessKit's winit adapter - Narrator and NVDA on
//! Windows, VoiceOver on a Mac, Orca on Linux. The core gathers the tree
//! once a frame inside `navigate_ui`; this sends it on whenever it changed,
//! and feeds the screen reader's "focus" and "press" back, which the next
//! `navigate_ui` carries out. Android and iOS are left out (no adapter).
//!
//! The adapter must exist before the window is first shown, so the runtime
//! creates its window hidden, builds this, and then shows it.

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop {
    use std::sync::{Arc, Mutex};

    use accesskit::{Action, ActionRequest, Live, Node, NodeId, Rect, Role, Toggled, TreeId, TreeInfo, TreeUpdate};
    use jengine_core::ui::accessibility as a11y;

    const ROOT: NodeId = NodeId(1);
    const LIVE: NodeId = NodeId(2);

    struct Activation(Arc<Mutex<Option<TreeUpdate>>>);

    impl accesskit::ActivationHandler for Activation {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            self.0.lock().ok()?.clone()
        }
    }

    /// The screen reader's requests, which may arrive on another thread:
    /// kept here until the main thread hands them to the core.
    struct Actions(Arc<Mutex<Vec<a11y::Action>>>);

    impl accesskit::ActionHandler for Actions {
        fn do_action(&mut self, r: ActionRequest) {
            let id = r.target_node.0;
            let action = match r.action {
                Action::Click => a11y::Action::Click(id),
                Action::Focus => a11y::Action::Focus(id),
                Action::Increment => a11y::Action::Step(id, 1),
                Action::Decrement => a11y::Action::Step(id, -1),
                _ => return,
            };
            if let Ok(mut q) = self.0.lock() {
                q.push(action);
            }
        }
    }

    struct Deactivation;

    impl accesskit::DeactivationHandler for Deactivation {
        fn deactivate_accessibility(&mut self) {}
    }

    pub struct Desktop {
        adapter: accesskit_winit::Adapter,
        latest: Arc<Mutex<Option<TreeUpdate>>>,
        actions: Arc<Mutex<Vec<a11y::Action>>>,
        sent: u64,
        title: String,
    }

    impl Desktop {
        pub fn new(event_loop: &winit::event_loop::ActiveEventLoop, window: &winit::window::Window, title: &str) -> Self {
            let latest = Arc::new(Mutex::new(Some(update(&a11y::Tree::default(), title))));
            let actions = Arc::new(Mutex::new(Vec::new()));
            let adapter = accesskit_winit::Adapter::with_direct_handlers(
                event_loop,
                window,
                Activation(Arc::clone(&latest)),
                Actions(Arc::clone(&actions)),
                Deactivation,
            );
            Self { adapter, latest, actions, sent: 0, title: title.to_string() }
        }

        pub fn process_event(&mut self, window: &winit::window::Window, event: &winit::event::WindowEvent) {
            self.adapter.process_event(window, event);
        }

        pub fn update(&mut self) {
            let asked: Vec<a11y::Action> = self.actions.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
            for a in asked {
                a11y::request(a);
            }
            let (tree, generation) = a11y::latest();
            if generation == self.sent {
                return;
            }
            self.sent = generation;
            let u = update(&tree, &self.title);
            if let Ok(mut l) = self.latest.lock() {
                *l = Some(u.clone());
            }
            self.adapter.update_if_active(|| u);
        }
    }

    /// The whole tree as one AccessKit update: a window holding a live
    /// region (announcements) and every node in reading order.
    fn update(tree: &a11y::Tree, title: &str) -> TreeUpdate {
        let mut root = Node::new(Role::Window);
        root.set_label(title);
        let mut live = Node::new(Role::Label);
        live.set_live(Live::Polite);
        // A label's words are its value in AccessKit (its `label` is for a
        // control's name), so static text and the live region carry both.
        live.set_value(tree.announcement.1.clone());
        let mut nodes = vec![(LIVE, live)];
        let mut children = vec![LIVE];
        for n in &tree.nodes {
            let id = NodeId(n.id);
            let mut node = Node::new(match n.role {
                a11y::Role::Button => Role::Button,
                a11y::Role::Label => Role::Label,
                a11y::Role::TextInput => Role::TextInput,
                a11y::Role::Toggle => Role::CheckBox,
                a11y::Role::Slider => Role::Slider,
                a11y::Role::ProgressBar => Role::ProgressIndicator,
                a11y::Role::Image => Role::Image,
            });
            node.set_label(n.label.clone());
            if n.role == a11y::Role::Label {
                node.set_value(n.label.clone());
            }
            if !n.description.is_empty() {
                node.set_description(n.description.clone());
            }
            if let Some(v) = &n.value {
                node.set_value(v.clone());
            }
            if let Some((v, lo, hi)) = n.numeric {
                node.set_numeric_value(v);
                node.set_min_numeric_value(lo);
                node.set_max_numeric_value(hi);
                node.set_numeric_value_step((hi - lo) * 0.1);
                if n.role == a11y::Role::Slider {
                    node.add_action(Action::Increment);
                    node.add_action(Action::Decrement);
                }
            }
            if let Some(t) = n.toggled {
                node.set_toggled(if t { Toggled::True } else { Toggled::False });
            }
            let [x, y, w, h] = n.bounds;
            node.set_bounds(Rect { x0: x as f64, y0: y as f64, x1: (x + w) as f64, y1: (y + h) as f64 });
            if n.focusable {
                node.add_action(Action::Focus);
                node.add_action(Action::Click);
            }
            nodes.push((id, node));
            children.push(id);
        }
        root.set_children(children);
        nodes.insert(0, (ROOT, root));
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: tree.focus.map(NodeId).unwrap_or(ROOT),
        }
    }
}

/// The runtime's side of the screen reader. Empty where there is no adapter.
pub(crate) struct NativeAccessibility {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    inner: desktop::Desktop,
}

impl NativeAccessibility {
    /// Build the adapter for `window`, which must not have been shown yet.
    pub(crate) fn new(event_loop: &ActiveEventLoop, window: &Window, title: &str) -> Self {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            Self { inner: desktop::Desktop::new(event_loop, window, title) }
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = (event_loop, window, title);
            Self {}
        }
    }

    /// Every window event, before the runtime's own handling.
    pub(crate) fn process_event(&mut self, window: &Window, event: &WindowEvent) {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        self.inner.process_event(window, event);
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let _ = (window, event);
    }

    /// Once a frame, after `navigate_ui`: the screen reader's requests to the
    /// core, and the tree to the screen reader if it changed.
    pub(crate) fn update(&mut self) {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        self.inner.update();
    }
}
