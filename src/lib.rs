//! Chromascope: a Toybox VST3 spectrum viewer with explicit companion sources.
//!
//! The bundle exposes two VST3 classes. `Chromascope` displays its own input and
//! selected companion traces; `Chromascope Companion` publishes the spectrum of
//! the track it is inserted on. Companion discovery is deliberately limited to
//! the in-process registry in [`registry`]; no host track or selection API is
//! involved.

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
#![allow(non_snake_case)]

mod alignment;
pub mod analysis;
pub mod constants;
pub mod gui;
pub mod registry;
pub mod render;
mod visual_system;

#[cfg(all(feature = "vst3", target_os = "macos"))]
mod presentation;

#[cfg(all(feature = "vst3", target_os = "macos"))]
mod radiant_editor;

#[cfg(feature = "vst3")]
mod instance_registry;
mod shared;

#[cfg(feature = "vst3")]
mod vst3;

pub use analysis::{SpectrumAnalyzer, SpectrumFrame};
pub use constants::{
    ALIGNMENT_MAX_SKEW_SAMPLES, DISPLAY_TRACE_SAMPLES, FRAME_HISTORY_LEN, MAIN_SPECTRUM_COLOR,
    MAX_BANDS, MAX_COMPANIONS, PRESENTATION_FRAME_SECONDS, PRESENTATION_TARGET_FPS,
};
pub use registry::{CompanionSourceSnapshot, SpectrumMailbox, snapshot_companions};
pub use shared::{ChromascopeShared, DeviceKind};
