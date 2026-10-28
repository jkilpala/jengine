//! Easing functions for tween animations.
//!
//! Each function maps `t` in [0..1] to a shaped output in [0..1].

use crate::detmath::DetMath;
use serde::{Deserialize, Serialize};

/// Available easing functions for tweens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EasingFunction {
    Linear,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,
}

impl EasingFunction {
    /// Evaluate the easing function at `t` (clamped to [0, 1]).
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseInQuad => t * t,
            Self::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Self::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).dpowi(2) / 2.0
                }
            }
            Self::EaseInCubic => t * t * t,
            Self::EaseOutCubic => 1.0 - (1.0 - t).dpowi(3),
            Self::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).dpowi(3) / 2.0
                }
            }
            Self::EaseInSine => {
                1.0 - (t * std::f32::consts::FRAC_PI_2).dcos()
            }
            Self::EaseOutSine => {
                (t * std::f32::consts::FRAC_PI_2).dsin()
            }
            Self::EaseInOutSine => {
                -(((t * std::f32::consts::PI).dcos() - 1.0) / 2.0)
            }
        }
    }

    /// Parse an easing name string (case-insensitive) to an EasingFunction.
    /// Returns `Linear` for unrecognized names.
    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "linear" => Self::Linear,
            // The short names are the quadratic curves. `docs/scripting-api.md`
            // has always shown `"ease_out"` and friends, and until Sep 2026 they
            // fell through to `Linear` with a warning.
            "easeinquad" | "ease_in_quad" | "ease_in" | "easein" => Self::EaseInQuad,
            "easeoutquad" | "ease_out_quad" | "ease_out" | "easeout" => Self::EaseOutQuad,
            "easeinoutquad" | "ease_in_out_quad" | "ease_in_out" | "easeinout" => Self::EaseInOutQuad,
            "easeincubic" | "ease_in_cubic" => Self::EaseInCubic,
            "easeoutcubic" | "ease_out_cubic" => Self::EaseOutCubic,
            "easeinoutcubic" | "ease_in_out_cubic" => Self::EaseInOutCubic,
            "easeinsine" | "ease_in_sine" => Self::EaseInSine,
            "easeoutsine" | "ease_out_sine" => Self::EaseOutSine,
            "easeinoutsine" | "ease_in_out_sine" => Self::EaseInOutSine,
            _ => {
                log::warn!("Unknown easing function '{}', defaulting to Linear", name);
                Self::Linear
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_endpoints() {
        let functions = [
            EasingFunction::Linear,
            EasingFunction::EaseInQuad,
            EasingFunction::EaseOutQuad,
            EasingFunction::EaseInOutQuad,
            EasingFunction::EaseInCubic,
            EasingFunction::EaseOutCubic,
            EasingFunction::EaseInOutCubic,
            EasingFunction::EaseInSine,
            EasingFunction::EaseOutSine,
            EasingFunction::EaseInOutSine,
        ];
        for f in functions {
            let at_0 = f.apply(0.0);
            let at_1 = f.apply(1.0);
            assert!(
                (at_0).abs() < 1e-6,
                "{:?}.apply(0.0) = {}, expected ~0",
                f,
                at_0
            );
            assert!(
                (at_1 - 1.0).abs() < 1e-6,
                "{:?}.apply(1.0) = {}, expected ~1",
                f,
                at_1
            );
        }
    }

    #[test]
    fn the_documented_short_names_are_the_quad_curves() {
        // `docs/scripting-api.md` shows these; they used to be Linear.
        assert_eq!(EasingFunction::from_name("ease_out"), EasingFunction::EaseOutQuad);
        assert_eq!(EasingFunction::from_name("ease_in"), EasingFunction::EaseInQuad);
        assert_eq!(EasingFunction::from_name("ease_in_out"), EasingFunction::EaseInOutQuad);
    }

    #[test]
    fn from_name_roundtrip() {
        assert_eq!(EasingFunction::from_name("Linear"), EasingFunction::Linear);
        assert_eq!(EasingFunction::from_name("EaseInQuad"), EasingFunction::EaseInQuad);
        assert_eq!(EasingFunction::from_name("ease_out_sine"), EasingFunction::EaseOutSine);
        assert_eq!(EasingFunction::from_name("unknown"), EasingFunction::Linear);
    }
}
