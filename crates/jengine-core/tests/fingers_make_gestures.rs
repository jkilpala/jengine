//! **Fingers make gestures** (Oct 2026; `input::gestures`,
//! `Input.get_gestures`). A game should not have to track raw fingers to
//! know a swipe from a pinch: `InputState::update_interactions` - which
//! every host calls once a frame before scripts - turns the frame's touches
//! (and the mouse, while no finger is down) into taps, double taps, long
//! presses, swipes, pans, pinches and twists. The recogniser is unit-tested
//! in its module; this drives it the way a host does, through `InputState`.
//! Found by the showcase's Photo Album.

use jengine_core::input::gestures::GestureKind;
use jengine_core::input::{InputState, Touch, TouchStage};

fn frame(input: &mut InputState, fingers: &[(u64, f32, f32, TouchStage)], now: f64) -> Vec<GestureKind> {
    input.touches = fingers
        .iter()
        .map(|&(id, x, y, stage)| Touch { id, position: (x, y), start: (x, y), stage, on_control: false })
        .collect();
    input.update_interactions_at(now);
    input.gestures.iter().map(|g| g.kind).collect()
}

#[test]
fn a_swipe_then_a_pinch_through_the_host_step() {
    let mut input = InputState::default();
    use TouchStage::*;
    // A flick to the left.
    frame(&mut input, &[(1, 600.0, 400.0, Began)], 0.0);
    assert_eq!(frame(&mut input, &[(1, 520.0, 400.0, Moved)], 0.05), [GestureKind::Pan]);
    let g = frame(&mut input, &[(1, 380.0, 405.0, Ended)], 0.12);
    assert_eq!(g, [GestureKind::Pan, GestureKind::Swipe]);
    let swipe = input.gestures.iter().find(|g| g.kind == GestureKind::Swipe).unwrap();
    assert!(swipe.dx < -200.0 && swipe.dy.abs() < 10.0, "left: {swipe:?}");
    // Nothing the next frame.
    assert!(frame(&mut input, &[], 0.2).is_empty());
    // Two fingers spread: a pinch growing, and no tap from either.
    frame(&mut input, &[(2, 300.0, 300.0, Began), (3, 400.0, 300.0, Began)], 1.0);
    frame(&mut input, &[(2, 250.0, 300.0, Moved), (3, 450.0, 300.0, Moved)], 1.1);
    let pinch = input.gestures.iter().find(|g| g.kind == GestureKind::Pinch).unwrap();
    assert!((pinch.scale - 2.0).abs() < 1e-3 && (pinch.x - 350.0).abs() < 1e-3, "{pinch:?}");
    let g = frame(&mut input, &[(2, 250.0, 300.0, Ended), (3, 450.0, 300.0, Ended)], 1.15);
    assert!(!g.contains(&GestureKind::Tap), "{g:?}");
}

#[test]
fn a_mouse_click_is_a_tap_when_no_finger_is_down() {
    let mut input = InputState::default();
    input.mouse_position = (40.0, 50.0);
    input.mouse_left = true;
    input.update_interactions_at(0.0);
    input.mouse_left = false;
    input.update_interactions_at(0.1);
    assert_eq!(input.gestures.iter().map(|g| g.kind).collect::<Vec<_>>(), [GestureKind::Tap]);
    assert_eq!((input.gestures[0].x, input.gestures[0].y), (40.0, 50.0));
}
