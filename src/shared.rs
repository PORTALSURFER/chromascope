//! Shared runtime state for a paired VST3 processor/controller instance.

use std::sync::Arc;

use crate::analysis::SpectrumFrame;
use crate::constants::MAX_BANDS;
#[cfg(feature = "vst3")]
use crate::constants::Rgb;
use crate::registry::{CompanionHandle, SpectrumMailbox, register_companion};

/// Which of Chromascope's two VST3 device classes owns a runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// Primary viewer showing its own input and selected companions.
    Viewer,
    /// Audio-track companion publishing one spectrum source.
    Companion,
}

/// State shared by a VST3 processor and its controller.
pub struct ChromascopeShared {
    /// Device class represented by this runtime.
    pub kind: DeviceKind,
    /// The viewer's own-input mailbox; unused by a companion.
    pub spectrum: Arc<SpectrumMailbox>,
    companion: Option<CompanionHandle>,
}

impl ChromascopeShared {
    /// Create a runtime and register a companion when requested.
    pub fn new(kind: DeviceKind) -> Self {
        Self {
            kind,
            spectrum: Arc::new(SpectrumMailbox::new()),
            companion: (kind == DeviceKind::Companion)
                .then(register_companion)
                .flatten(),
        }
    }

    /// Publish one frame to the correct destination mailbox.
    pub fn publish_spectrum(&self, values: &[f32; MAX_BANDS]) {
        self.publish_frame(&SpectrumFrame {
            sequence: 0,
            sample_position: None,
            values: *values,
        });
    }

    /// Publish one frame, preserving its optional host-timeline position.
    pub fn publish_frame(&self, frame: &SpectrumFrame) {
        match self.companion.as_ref() {
            Some(companion) if companion.analysis_requested() => {
                let _ = companion.publish_frame(frame);
            }
            Some(_) => {}
            None => {
                let _ = self.spectrum.publish_frame(frame);
            }
        }
    }

    /// Change companion activity without taking a registry lock.
    pub fn set_active(&self, active: bool) {
        if let Some(companion) = self.companion.as_ref() {
            companion.set_active(active);
        }
    }

    /// Return the assigned companion id when this is a registered companion.
    pub fn companion_id(&self) -> Option<u64> {
        self.companion.as_ref().map(CompanionHandle::id)
    }

    /// Return the assigned generated fallback color when this is a registered companion.
    pub fn companion_color(&self) -> Option<crate::constants::Rgb> {
        self.companion.as_ref().map(CompanionHandle::color)
    }

    /// Return whether this companion has a viewer requesting analysis.
    pub fn companion_analysis_requested(&self) -> bool {
        self.companion
            .as_ref()
            .is_some_and(CompanionHandle::analysis_requested)
    }

    /// Apply a host-provided companion track name outside the audio callback.
    #[cfg(feature = "vst3")]
    pub(crate) fn set_host_name(&self, name: Option<String>) {
        if let Some(companion) = self.companion.as_ref() {
            companion.set_host_name(name);
        }
    }

    /// Apply a host-provided companion track color outside the audio callback.
    #[cfg(feature = "vst3")]
    pub(crate) fn set_host_color(&self, color: Option<Rgb>) {
        if let Some(companion) = self.companion.as_ref() {
            companion.set_host_color(color);
        }
    }
}

impl Drop for ChromascopeShared {
    fn drop(&mut self) {
        if let Some(companion) = self.companion.as_ref() {
            companion.set_active(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::MAX_BANDS;
    use crate::registry::{set_companion_analysis_interest, snapshot_companions};

    #[test]
    fn companion_shared_publishes_only_for_requested_analysis() {
        let shared = Arc::new(ChromascopeShared::new(DeviceKind::Companion));
        let id = shared
            .companion_id()
            .expect("companion runtime should have a registry id");
        let values = [2.0; MAX_BANDS];

        shared.publish_spectrum(&values);
        let idle = snapshot_companions()
            .into_iter()
            .find(|source| source.id == id)
            .expect("idle companion should remain discoverable");
        assert!(!idle.analysis_requested);
        assert!(idle.frame.is_none());

        assert!(set_companion_analysis_interest(id, true));
        shared.publish_spectrum(&values);
        let live = snapshot_companions()
            .into_iter()
            .find(|source| source.id == id)
            .expect("requested companion should remain discoverable");
        assert!(live.analysis_requested);
        assert!(live.frame.is_some());
        assert!(set_companion_analysis_interest(id, false));
    }
}
