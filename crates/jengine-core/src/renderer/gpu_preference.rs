//! **Which GPU a game asks for** (`settings.gpu_preference` in
//! `project.jengine`, Oct 2026): on a machine with two - a laptop's
//! integrated and discrete GPUs - `high_performance` (the default) asks for
//! the faster one and `low_power` for the one that saves battery.
//!
//! It is a request: wgpu passes it to the platform, which natively picks the
//! adapter as asked, and in a browser may ignore it. Before this the choice
//! was `HighPerformance` in code, so a small game could not ask to leave a
//! laptop's discrete GPU asleep. The editor's own GPU is not a project's to
//! choose - it is the editor's preference (`--force_high_performance_gpu`),
//! since the editor's window exists before any project is open.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuPreference {
    /// The discrete GPU where there is one.
    #[default]
    HighPerformance,
    /// The integrated GPU where there is one.
    LowPower,
}

impl GpuPreference {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// `gpu_preference` from a project's `settings` object; absent or
    /// unknown is the default.
    pub fn from_settings(settings: Option<&serde_json::Value>) -> Self {
        settings
            .and_then(|s| s.get("gpu_preference"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    #[cfg(feature = "gpu-wgpu")]
    pub fn to_wgpu(self) -> wgpu::PowerPreference {
        match self {
            Self::HighPerformance => wgpu::PowerPreference::HighPerformance,
            Self::LowPower => wgpu::PowerPreference::LowPower,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_without_one_asks_for_the_fast_gpu() {
        assert_eq!(GpuPreference::from_settings(None), GpuPreference::HighPerformance);
        let s = serde_json::json!({ "width": 1280 });
        assert_eq!(GpuPreference::from_settings(Some(&s)), GpuPreference::HighPerformance);
    }

    #[test]
    fn a_project_can_ask_for_the_battery_saving_gpu() {
        let s = serde_json::json!({ "gpu_preference": "low_power" });
        assert_eq!(GpuPreference::from_settings(Some(&s)), GpuPreference::LowPower);
        let bad = serde_json::json!({ "gpu_preference": "fastest" });
        assert_eq!(GpuPreference::from_settings(Some(&bad)), GpuPreference::HighPerformance, "unknown is the default");
    }
}
