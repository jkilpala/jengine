//! Pointer-driven scrolling for GUI containers: drag, flick, and elastic ends.
//!
//! # What this adds
//!
//! An `Overflow::Scroll` container could be scrolled by the mouse wheel
//! ([`crate::ui::layout::apply_gui_scroll`]) or by a script writing `scroll_y`.
//! Neither is a way to *grab the list and pull it*, which is how every touch
//! surface and most desktop UIs work — and on a phone the wheel does not exist
//! at all.
//!
//! So: press inside a scrolling container and the content follows the pointer;
//! release with speed and it keeps going and slows down; pull past either end
//! and it resists, then springs back when let go. Web already maps touch events
//! onto the mouse fields, so building the gesture on those gets touch for free.
//!
//! # Why the elastic offset is a separate field
//!
//! `NodeStyle::scroll_y` is authored, serialized, and clamped to the scrollable
//! range by layout — `gui_set(id, "scroll_y", 99999)` is how a script says "jump
//! to the end", and that only works because layout clamps. A rubber band has to
//! go *outside* that range, so it cannot live in the same field without either
//! breaking the clamp or corrupting the authored value.
//!
//! [`NodeStyle::overscroll_y`] is the runtime-only companion: not serialized,
//! added by layout after the clamp. `scroll_y` stays the honest position and
//! the overscroll is the transient stretch.
//!
//! # Feel
//!
//! The rubber band is the standard asymptotic curve — pulling twice as hard
//! past the end does not move twice as far, and the offset can never exceed
//! `viewport * RUBBER_LIMIT` however hard you pull, so there is no way to fling
//! the content off screen. Spring-back and friction are exponential rather than
//! spring-mass: stable at any frame time, including the long first frame after
//! a scene load, where a stiff spring integrated with a 200 ms step would
//! overshoot wildly.

use std::collections::HashMap;

use crate::ui::gui_document::GuiDocument;
use crate::ui::layout::{LayoutResult, compute_layout};

/// Fraction of the viewport the content can be pulled past its end.
///
/// The rubber curve approaches this asymptotically, so it is a hard ceiling
/// rather than a threshold.
const RUBBER_LIMIT: f32 = 0.55;

/// How fast an overscrolled container returns to its bound, per second.
///
/// Exponential: the fraction remaining after `dt` is `exp(-SPRING_RATE * dt)`.
const SPRING_RATE: f32 = 14.0;

/// How fast a flick loses speed, per second, as an exponential decay.
const FRICTION: f32 = 5.0;

/// Below this speed (px/s) a flick is over.
const MIN_VELOCITY: f32 = 12.0;

/// Below this (px) an overscroll is finished and snaps to exactly zero.
const MIN_OVERSCROLL: f32 = 0.25;

/// How much of the newest pointer sample decides the flick velocity.
///
/// Low enough to ignore a single jittery frame, high enough that the release
/// direction is the one the finger was last going — averaging the whole drag
/// would flick backwards after a there-and-back gesture.
const VELOCITY_SMOOTHING: f32 = 0.7;

/// Which axis a container scrolls on.
///
/// A container with both axes scrollable is driven vertically, matching
/// `apply_gui_scroll` — one rule, so the wheel and a drag never disagree about
/// what a list does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

/// One pointer sample, as every runtime already has it.
#[derive(Debug, Clone, Copy)]
pub struct PointerInput {
    pub x: f32,
    pub y: f32,
    /// Whether the primary button (or a finger) is down right now.
    pub down: bool,
    pub just_pressed: bool,
    pub just_released: bool,
}

#[derive(Debug, Clone)]
struct ActiveDrag {
    gui_id: String,
    node_id: String,
    axis: Axis,
    /// Pointer coordinate on the drag axis when the grab started.
    grab_pointer: f32,
    /// The container's scroll offset when the grab started.
    grab_scroll: f32,
    last_pointer: f32,
    max: f32,
    /// Container extent along the axis, which sets the rubber band's ceiling.
    viewport: f32,
    /// Whether the pointer has moved far enough to be a scroll rather than a
    /// click. Until it has, the gesture is not claimed.
    committed: bool,
}

/// Pointer travel (px) before a press becomes a drag rather than a click.
///
/// Without it, every button press inside a scrolling list would be a
/// zero-distance drag, and a click that wobbles by a pixel would scroll.
const DRAG_SLOP: f32 = 4.0;

/// Per-container motion that outlives the gesture that started it.
#[derive(Debug, Default, Clone, Copy)]
struct Motion {
    velocity: f32,
}

/// Drag, flick and elastic state for every scrolling container a runtime shows.
///
/// One per runtime, ticked once a frame. Small: it holds an entry only for
/// containers that are actually moving.
#[derive(Debug, Default)]
pub struct GuiScrollState {
    active: Option<ActiveDrag>,
    motion: HashMap<(String, String), Motion>,
    /// A committed drag ended on this pointer-up and its release is spent.
    ///
    /// The release *is* the end of the drag, so on that frame `active` has
    /// already been cleared — and asking "is a drag in progress?" then answers
    /// no, which let the click fire after every scroll. Set here until the next
    /// press, which is safe because a click only ever fires on the release edge
    /// and a release is always preceded by a press.
    consumed_release: bool,
}

impl GuiScrollState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a scroll gesture owns the pointer, **including the frame its
    /// release lands on**.
    ///
    /// A runtime suppresses the click it would otherwise deliver: letting go
    /// after dragging a list must not also press the button under the finger.
    /// That release is precisely when the drag ends, so this stays true for it
    /// — reporting the truthful "no drag is active any more" was the whole bug.
    pub fn is_dragging(&self) -> bool {
        self.consumed_release || self.active.as_ref().is_some_and(|d| d.committed)
    }

    /// Advance one document by one frame.
    ///
    /// Returns `true` when anything moved, so a caller can skip a redraw.
    /// Call for every visible document; the drag is exclusive across documents,
    /// and a document that is not the drag target still gets its spring and
    /// flick advanced.
    pub fn update(
        &mut self,
        doc: &mut GuiDocument,
        gui_id: &str,
        pointer: PointerInput,
        screen: (f32, f32),
        dt: f32,
    ) -> bool {
        if screen.0 <= 0.0 || screen.1 <= 0.0 {
            return false;
        }
        let layout = compute_layout(doc, screen.0, screen.1);
        let mut moved = clamp_stored_offsets(doc, &layout);

        if pointer.just_pressed {
            // A new press starts a new gesture: whatever the last one consumed
            // is spent.
            self.consumed_release = false;
            if self.active.is_none() {
                moved |= self.begin(doc, gui_id, &layout, pointer);
            }
        }
        if self.active.as_ref().is_some_and(|d| d.gui_id == gui_id) {
            if pointer.down && !pointer.just_released {
                moved |= self.drag(doc, pointer, dt);
            } else {
                self.release();
            }
        }
        moved |= self.settle(doc, gui_id, &layout, dt);
        moved
    }

    /// Abandon any in-progress drag.
    ///
    /// For a runtime whose pointer has left the window or whose document has
    /// been swapped out from under the gesture.
    pub fn cancel(&mut self) {
        self.active = None;
        self.consumed_release = false;
    }

    fn begin(
        &mut self,
        doc: &GuiDocument,
        gui_id: &str,
        layout: &LayoutResult,
        pointer: PointerInput,
    ) -> bool {
        // Deepest hit wins, exactly as the wheel does: entries are emitted
        // parent-before-child, so the last match is the innermost container.
        let Some(entry) = layout
            .iter()
            .rev()
            .find(|e| e.scroll_max.is_some() && e.contains(pointer.x, pointer.y))
        else {
            return false;
        };
        let (max_x, max_y) = entry.scroll_max.unwrap_or((0.0, 0.0));
        let (axis, max, viewport) = if max_y > 0.0 {
            (Axis::Y, max_y, entry.rect.height)
        } else if max_x > 0.0 {
            (Axis::X, max_x, entry.rect.width)
        } else {
            // Scrollable in principle, nothing to scroll to.
            return false;
        };
        let Some(node) = doc.root.find_node(&entry.id) else {
            return false;
        };
        let (grab_scroll, pointer_v) = match axis {
            Axis::Y => (node.style.scroll_y, pointer.y),
            Axis::X => (node.style.scroll_x, pointer.x),
        };
        // A press stops a flick in progress, the way catching a spinning wheel
        // does. Without this, grabbing a coasting list fights it.
        self.motion.insert((gui_id.to_string(), entry.id.clone()), Motion::default());
        self.active = Some(ActiveDrag {
            gui_id: gui_id.to_string(),
            node_id: entry.id.clone(),
            axis,
            grab_pointer: pointer_v,
            grab_scroll,
            last_pointer: pointer_v,
            max,
            viewport: viewport.max(1.0),
            committed: false,
        });
        false
    }

    fn drag(&mut self, doc: &mut GuiDocument, pointer: PointerInput, dt: f32) -> bool {
        let Some(drag) = self.active.as_mut() else {
            return false;
        };
        let now = match drag.axis {
            Axis::Y => pointer.y,
            Axis::X => pointer.x,
        };
        if !drag.committed {
            if (now - drag.grab_pointer).abs() < DRAG_SLOP {
                drag.last_pointer = now;
                return false;
            }
            drag.committed = true;
            // Re-anchor by exactly the slop, not to the current position.
            //
            // Anchoring to `now` would discard the whole first movement, and a
            // fast drag arrives as one big delta — so the gesture would commit
            // and scroll nothing that frame. Subtracting only the slop keeps
            // the content under the finger while still not jumping by the few
            // pixels that decided this was a scroll.
            drag.grab_pointer += (now - drag.grab_pointer).signum() * DRAG_SLOP;
        }

        // The content follows the pointer: dragging down reveals earlier
        // content, which is a *smaller* scroll offset.
        let desired = drag.grab_scroll - (now - drag.grab_pointer);
        let (clamped, over) = split_overscroll(desired, drag.max, drag.viewport);

        let key = (drag.gui_id.clone(), drag.node_id.clone());
        let delta = now - drag.last_pointer;
        drag.last_pointer = now;
        if dt > 0.0 {
            let instant = -delta / dt;
            let m = self.motion.entry(key).or_default();
            m.velocity = m.velocity * (1.0 - VELOCITY_SMOOTHING) + instant * VELOCITY_SMOOTHING;
        }

        let Some(drag) = self.active.as_ref() else {
            return false;
        };
        write_scroll(doc, &drag.node_id, drag.axis, clamped, over)
    }

    fn release(&mut self) {
        // Only a *committed* drag eats its release. A press that never moved
        // is an ordinary click and must still reach the button under it.
        if let Some(drag) = self.active.take() {
            self.consumed_release |= drag.committed;
        }
    }

    /// Advance flick and spring-back for every moving container in `doc`.
    fn settle(
        &mut self,
        doc: &mut GuiDocument,
        gui_id: &str,
        layout: &LayoutResult,
        dt: f32,
    ) -> bool {
        if dt <= 0.0 {
            return false;
        }
        let dragging_here = self
            .active
            .as_ref()
            .filter(|d| d.gui_id == gui_id && d.committed)
            .map(|d| d.node_id.clone());

        let mut moved = false;
        for entry in layout.iter() {
            let Some((max_x, max_y)) = entry.scroll_max else { continue };
            if dragging_here.as_deref() == Some(entry.id.as_str()) {
                continue;
            }
            let (axis, max, viewport) = if max_y > 0.0 {
                (Axis::Y, max_y, entry.rect.height)
            } else if max_x > 0.0 {
                (Axis::X, max_x, entry.rect.width)
            } else {
                (Axis::Y, 0.0, entry.rect.height)
            };
            let key = (gui_id.to_string(), entry.id.clone());
            let Some(node) = doc.root.find_node(&entry.id) else { continue };
            let (scroll, over) = match axis {
                Axis::Y => (node.style.scroll_y, node.style.overscroll_y),
                Axis::X => (node.style.scroll_x, node.style.overscroll_x),
            };
            let velocity = self.motion.get(&key).map(|m| m.velocity).unwrap_or(0.0);
            if velocity == 0.0 && over == 0.0 {
                continue;
            }

            let (next_scroll, next_over, next_velocity) = step_motion(
                scroll,
                over,
                velocity,
                max,
                viewport.max(1.0),
                dt,
            );
            if next_velocity == 0.0 {
                self.motion.remove(&key);
            } else {
                self.motion.insert(key, Motion { velocity: next_velocity });
            }
            moved |= write_scroll(doc, &entry.id, axis, next_scroll, next_over);
        }
        moved
    }
}

/// Split a desired offset into the part inside the scrollable range and the
/// rubber-banded remainder.
fn split_overscroll(desired: f32, max: f32, viewport: f32) -> (f32, f32) {
    if desired < 0.0 {
        (0.0, -rubber(-desired, viewport))
    } else if desired > max {
        (max, rubber(desired - max, viewport))
    } else {
        (desired.clamp(0.0, max), 0.0)
    }
}

/// The rubber-band curve: `distance` of pull becomes this much movement.
///
/// Asymptotic — it approaches `viewport * RUBBER_LIMIT` and never reaches it,
/// so no amount of pulling detaches the content from the container. Standard
/// shape, the same one touch platforms use.
fn rubber(distance: f32, viewport: f32) -> f32 {
    let limit = viewport * RUBBER_LIMIT;
    if limit <= 0.0 {
        return 0.0;
    }
    (distance * limit) / (limit + distance)
}

/// One frame of flick and spring for a container nobody is holding.
fn step_motion(
    scroll: f32,
    over: f32,
    velocity: f32,
    max: f32,
    viewport: f32,
    dt: f32,
) -> (f32, f32, f32) {
    let mut scroll = scroll;
    let mut over = over;
    let mut velocity = velocity;

    if over != 0.0 {
        // Springing back: the flick is over, the spring owns the motion. Left
        // together they fight, and a flick into the end oscillates.
        velocity = 0.0;
        over *= (-SPRING_RATE * dt).exp();
        if over.abs() < MIN_OVERSCROLL {
            over = 0.0;
        }
    } else if velocity != 0.0 {
        scroll += velocity * dt;
        velocity *= (-FRICTION * dt).exp();
        if velocity.abs() < MIN_VELOCITY {
            velocity = 0.0;
        }
        if scroll < 0.0 {
            // Coasting into the end: hand the remaining speed to the spring as
            // a stretch, so a flick bounces instead of stopping dead.
            over = -rubber(-scroll, viewport).min(viewport * RUBBER_LIMIT);
            scroll = 0.0;
            velocity = 0.0;
        } else if scroll > max {
            over = rubber(scroll - max, viewport).min(viewport * RUBBER_LIMIT);
            scroll = max;
            velocity = 0.0;
        }
    }
    (scroll, over, velocity)
}

/// Bring every stored `scroll_x` / `scroll_y` back inside its container's
/// range.
///
/// Layout clamps the offset it *uses* but never wrote it back, so
/// `gui_set(gui, node, "scroll_y", 99999)` - the documented way to say "jump to
/// the end" - drew at the bottom while `gui_get` kept answering 99999. Worse,
/// the next relative move started from there: a `-100` after "End" set 99899,
/// which still draws at the bottom, so the button appeared to do nothing
/// (test-lab scene 33, BUGS #97). Called once per frame per visible document,
/// from the same update every runtime already makes.
pub fn clamp_stored_offsets(doc: &mut GuiDocument, layout: &LayoutResult) -> bool {
    let mut changed = false;
    for entry in layout {
        let Some((max_x, max_y)) = entry.scroll_max else { continue };
        let Some(node) = doc.root.find_node_mut(&entry.id) else { continue };
        let (x, y) = (node.style.scroll_x.clamp(0.0, max_x), node.style.scroll_y.clamp(0.0, max_y));
        if x != node.style.scroll_x || y != node.style.scroll_y {
            node.style.scroll_x = x;
            node.style.scroll_y = y;
            changed = true;
        }
    }
    changed
}

/// Write an offset pair back to a node, reporting whether anything changed.
fn write_scroll(doc: &mut GuiDocument, node_id: &str, axis: Axis, scroll: f32, over: f32) -> bool {
    let Some(node) = doc.root.find_node_mut(node_id) else {
        return false;
    };
    let (before_s, before_o) = match axis {
        Axis::Y => (node.style.scroll_y, node.style.overscroll_y),
        Axis::X => (node.style.scroll_x, node.style.overscroll_x),
    };
    match axis {
        Axis::Y => {
            node.style.scroll_y = scroll;
            node.style.overscroll_y = over;
        }
        Axis::X => {
            node.style.scroll_x = scroll;
            node.style.overscroll_x = over;
        }
    }
    before_s != scroll || before_o != over
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scrollable document to drive gestures against: a 100px window over
    /// 300px of content.
    fn scroll_doc() -> GuiDocument {
        crate::ui::gui_document::load_gui(
            r#"{
              "name": "t", "anchor": "TopLeft",
              "root": {
                "id": "list", "node_type": { "type": "Panel" },
                "style": { "width": {"type":"Fixed","value":200.0},
                           "height": {"type":"Fixed","value":100.0},
                           "overflow": "Scroll" },
                "children": [
                  { "id": "inner", "node_type": { "type": "Panel" },
                    "style": { "width": {"type":"Fixed","value":200.0},
                               "height": {"type":"Fixed","value":300.0} },
                    "children": [] }
                ]
              }
            }"#,
        )
        .expect("parse")
    }

    fn press(x: f32, y: f32) -> PointerInput {
        PointerInput { x, y, down: true, just_pressed: true, just_released: false }
    }
    fn hold(x: f32, y: f32) -> PointerInput {
        PointerInput { x, y, down: true, just_pressed: false, just_released: false }
    }
    fn lift(x: f32, y: f32) -> PointerInput {
        PointerInput { x, y, down: false, just_pressed: false, just_released: true }
    }

    /// "Jump to the end" is `scroll_y = 99999`; what is stored afterwards must
    /// be the real end, so reading it back is honest and a relative move from
    /// it moves (BUGS #97).
    #[test]
    fn an_out_of_range_offset_is_stored_back_in_range() {
        let mut doc = scroll_doc();
        doc.root.find_node_mut("list").unwrap().style.scroll_y = 99999.0;
        let mut st = GuiScrollState::new();
        st.update(&mut doc, "t", hold(500.0, 500.0), (400.0, 400.0), 1.0 / 60.0);
        assert_eq!(doc.root.find_node_mut("list").unwrap().style.scroll_y, 200.0);

        doc.root.find_node_mut("list").unwrap().style.scroll_y = -50.0;
        st.update(&mut doc, "t", hold(500.0, 500.0), (400.0, 400.0), 1.0 / 60.0);
        assert_eq!(doc.root.find_node_mut("list").unwrap().style.scroll_y, 0.0);
    }

    /// The bug this was reported for: letting go after a scroll also clicked
    /// the button under the finger.
    ///
    /// The release *is* the end of the drag, so on that frame the gesture has
    /// already cleared itself — and a truthful "no drag is active" let the
    /// click through every single time.
    #[test]
    fn a_committed_drag_still_owns_the_pointer_on_the_frame_it_is_released() {
        let mut doc = scroll_doc();
        let mut st = GuiScrollState::new();
        let screen = (400.0, 400.0);
        let dt = 1.0 / 60.0;

        st.update(&mut doc, "t", press(50.0, 50.0), screen, dt);
        // Past the slop, so this is a scroll rather than a click.
        st.update(&mut doc, "t", hold(50.0, 20.0), screen, dt);
        assert!(st.is_dragging(), "a 30px drag should have been claimed");

        st.update(&mut doc, "t", lift(50.0, 20.0), screen, dt);
        assert!(
            st.is_dragging(),
            "the release that ended the drag was not consumed — this is exactly              the frame the click fires on, so the list scrolls *and* the button              under the finger activates"
        );
    }

    /// And a tap must still click. Consuming every release would trade one bug
    /// for a worse one: an unclickable menu.
    #[test]
    fn a_tap_that_never_moved_leaves_its_release_alone() {
        let mut doc = scroll_doc();
        let mut st = GuiScrollState::new();
        let screen = (400.0, 400.0);
        let dt = 1.0 / 60.0;

        st.update(&mut doc, "t", press(50.0, 50.0), screen, dt);
        // Inside the slop: a hand wobbling on a button, not a scroll.
        st.update(&mut doc, "t", hold(50.0, 48.0), screen, dt);
        st.update(&mut doc, "t", lift(50.0, 48.0), screen, dt);
        assert!(
            !st.is_dragging(),
            "a press that never travelled ate its own click"
        );
    }

    /// The consumed release must not outlive the gesture that spent it.
    #[test]
    fn the_next_press_clears_the_consumed_release() {
        let mut doc = scroll_doc();
        let mut st = GuiScrollState::new();
        let screen = (400.0, 400.0);
        let dt = 1.0 / 60.0;

        st.update(&mut doc, "t", press(50.0, 50.0), screen, dt);
        st.update(&mut doc, "t", hold(50.0, 10.0), screen, dt);
        st.update(&mut doc, "t", lift(50.0, 10.0), screen, dt);
        assert!(st.is_dragging());

        // A fresh tap somewhere harmless.
        st.update(&mut doc, "t", press(50.0, 50.0), screen, dt);
        st.update(&mut doc, "t", lift(50.0, 50.0), screen, dt);
        assert!(
            !st.is_dragging(),
            "a stale consumed release survived into the next gesture, so the              first click after any scroll is swallowed"
        );
    }

    /// Dragging really scrolls, not just claims the gesture.
    #[test]
    fn dragging_up_scrolls_the_content_down() {
        let mut doc = scroll_doc();
        let mut st = GuiScrollState::new();
        let screen = (400.0, 400.0);
        let dt = 1.0 / 60.0;

        st.update(&mut doc, "t", press(50.0, 60.0), screen, dt);
        st.update(&mut doc, "t", hold(50.0, 20.0), screen, dt);
        let scrolled = doc.root.find_node("list").expect("list").style.scroll_y;
        assert!(
            scrolled > 0.0,
            "dragging the content upward should reveal later content; scroll_y = {scrolled}"
        );
    }

    #[test]
    fn the_rubber_band_never_exceeds_its_limit() {
        let viewport = 200.0;
        let ceiling = viewport * RUBBER_LIMIT;
        for pull in [1.0f32, 50.0, 500.0, 50_000.0, 1.0e9] {
            let got = rubber(pull, viewport);
            assert!(
                got < ceiling,
                "pulling {pull} gave {got}, which is past the {ceiling} ceiling — \
                 the content can be detached from its container"
            );
        }
    }

    #[test]
    fn the_rubber_band_resists_progressively() {
        let v = 200.0;
        // Twice the pull must not give twice the movement, or it is not a
        // rubber band, it is a straight line.
        assert!(rubber(100.0, v) < rubber(50.0, v) * 2.0);
        assert!(rubber(50.0, v) > 0.0);
    }

    #[test]
    fn inside_the_range_there_is_no_overscroll() {
        let (s, o) = split_overscroll(40.0, 100.0, 200.0);
        assert_eq!((s, o), (40.0, 0.0));
    }

    #[test]
    fn past_each_end_the_offset_is_rubber_banded() {
        let (s, o) = split_overscroll(-30.0, 100.0, 200.0);
        assert_eq!(s, 0.0);
        assert!(o < 0.0 && o > -30.0, "expected a damped negative offset, got {o}");

        let (s, o) = split_overscroll(160.0, 100.0, 200.0);
        assert_eq!(s, 100.0);
        assert!(o > 0.0 && o < 60.0, "expected a damped positive offset, got {o}");
    }

    #[test]
    fn a_stretch_springs_back_to_zero() {
        let (mut scroll, mut over, mut vel) = (0.0f32, -40.0f32, 0.0f32);
        for _ in 0..120 {
            (scroll, over, vel) = step_motion(scroll, over, vel, 100.0, 200.0, 1.0 / 60.0);
        }
        assert_eq!(over, 0.0, "the container never returned to rest");
        assert_eq!(scroll, 0.0);
    }

    #[test]
    fn a_flick_coasts_and_stops() {
        let (mut scroll, mut over, mut vel) = (10.0f32, 0.0f32, 300.0f32);
        let mut steps = 0;
        while vel != 0.0 && steps < 600 {
            (scroll, over, vel) = step_motion(scroll, over, vel, 1000.0, 200.0, 1.0 / 60.0);
            steps += 1;
        }
        assert_eq!(vel, 0.0, "the flick never stopped");
        assert!(scroll > 10.0, "the flick did not move anything");
        assert!(scroll < 1000.0, "the flick ran past the end without being caught");
    }

    #[test]
    fn a_flick_into_the_end_bounces_rather_than_stopping_dead() {
        // Fast enough to reach the end within a frame or two.
        let (mut scroll, mut over, mut vel) = (95.0f32, 0.0f32, 900.0f32);
        (scroll, over, vel) = step_motion(scroll, over, vel, 100.0, 200.0, 1.0 / 60.0);
        assert_eq!(scroll, 100.0, "it should be pinned at the end");
        assert!(over > 0.0, "the leftover speed should have become a stretch");
        assert_eq!(vel, 0.0, "the spring owns the motion once stretched");

        // And it comes back.
        for _ in 0..120 {
            (scroll, over, vel) = step_motion(scroll, over, vel, 100.0, 200.0, 1.0 / 60.0);
        }
        assert_eq!(over, 0.0);
    }

    /// A long frame — the first one after a scene load, say — must not throw
    /// the content somewhere absurd. Exponential decay is chosen for exactly
    /// this: `exp(-k*dt)` is bounded for any `dt`, where a stiff spring
    /// integrated explicitly is not.
    #[test]
    fn a_very_long_frame_stays_stable() {
        let (mut scroll, mut over, mut vel) = (50.0f32, -60.0f32, 400.0f32);
        for dt in [0.25f32, 1.0, 4.0] {
            (scroll, over, vel) = step_motion(scroll, over, vel, 100.0, 200.0, dt);
            assert!(scroll.is_finite() && over.is_finite() && vel.is_finite());
            assert!(over.abs() <= 60.0, "the stretch grew during a long frame: {over}");
            assert!((0.0..=100.0).contains(&scroll), "scroll left its range: {scroll}");
        }
    }
}
