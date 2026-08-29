//! Realtime-safe VST3 audio processors for the viewer and companion classes.

use std::cell::UnsafeCell;
use std::sync::Arc;

use toybox::vst3::prelude::Steinberg::*;
use toybox::vst3::prelude::*;

use crate::analysis::SpectrumAnalyzer;
use crate::instance_registry::{SharedRole, release_shared_for_role};
use crate::shared::{ChromascopeShared, DeviceKind};

use super::{COMPANION_CONTROLLER_CID, VIEWER_CONTROLLER_CID, vst3_bus_flag};

/// One stereo processor paired with a shared viewer or companion runtime.
pub(super) struct ChromascopeVst3Processor {
    kind: DeviceKind,
    shared: Arc<ChromascopeShared>,
    analyzer: UnsafeCell<SpectrumAnalyzer>,
    analysis_requested: UnsafeCell<bool>,
}

// VST3 calls the processor through a shared COM reference, while the host
// serializes setup/activation with process callbacks. The analyzer is only
// mutated by that one processor's lifecycle and process path.
unsafe impl Send for ChromascopeVst3Processor {}
unsafe impl Sync for ChromascopeVst3Processor {}

impl ChromascopeVst3Processor {
    /// Create a processor with all FFT storage allocated before audio starts.
    pub(super) fn new(kind: DeviceKind, shared: Arc<ChromascopeShared>) -> Self {
        Self {
            kind,
            shared,
            analyzer: UnsafeCell::new(SpectrumAnalyzer::new(48_000.0)),
            analysis_requested: UnsafeCell::new(kind == DeviceKind::Viewer),
        }
    }

    fn should_analyze(&self) -> bool {
        match self.kind {
            DeviceKind::Viewer => true,
            DeviceKind::Companion => self.shared.companion_analysis_requested(),
        }
    }
}

impl Drop for ChromascopeVst3Processor {
    fn drop(&mut self) {
        self.shared.set_active(false);
        release_shared_for_role(&self.shared, SharedRole::Processor);
    }
}

impl Class for ChromascopeVst3Processor {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements);
}

impl IPluginBaseTrait for ChromascopeVst3Processor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for ChromascopeVst3Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        if class_id.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            *class_id = match self.kind {
                DeviceKind::Viewer => VIEWER_CONTROLLER_CID,
                DeviceKind::Companion => COMPANION_CONTROLLER_CID,
            };
        }
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, dir: BusDirection) -> i32 {
        match media_type as MediaTypes {
            MediaTypes_::kAudio => match dir as BusDirections {
                BusDirections_::kInput | BusDirections_::kOutput => 1,
                _ => 0,
            },
            _ => 0,
        }
    }

    unsafe fn getBusInfo(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if bus.is_null() || index != 0 || media_type as MediaTypes != MediaTypes_::kAudio {
            return kInvalidArgument;
        }
        let label = match dir as BusDirections {
            BusDirections_::kInput => {
                if self.kind == DeviceKind::Companion {
                    "Companion Input"
                } else {
                    "Main Input"
                }
            }
            BusDirections_::kOutput => "Output",
            _ => return kInvalidArgument,
        };
        let bus = unsafe { &mut *bus };
        bus.mediaType = MediaTypes_::kAudio as MediaType;
        bus.direction = dir;
        bus.channelCount = 2;
        copy_wstring(label, &mut bus.name);
        bus.busType = BusTypes_::kMain as BusType;
        bus.flags = vst3_bus_flag(BusInfo_::BusFlags_::kDefaultActive);
        kResultOk
    }

    unsafe fn getRoutingInfo(
        &self,
        _in_info: *mut RoutingInfo,
        _out_info: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: i32,
        _state: TBool,
    ) -> tresult {
        if media_type as MediaTypes != MediaTypes_::kAudio
            || index != 0
            || !matches!(
                dir as BusDirections,
                BusDirections_::kInput | BusDirections_::kOutput
            )
        {
            return kInvalidArgument;
        }
        kResultOk
    }

    unsafe fn setActive(&self, state: TBool) -> tresult {
        self.shared.set_active(state != 0);
        kResultOk
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
}

impl IAudioProcessorTrait for ChromascopeVst3Processor {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        num_ins: i32,
        outputs: *mut SpeakerArrangement,
        num_outs: i32,
    ) -> tresult {
        if num_ins != 1 || num_outs != 1 || inputs.is_null() || outputs.is_null() {
            return kInvalidArgument;
        }
        if unsafe { *inputs } != SpeakerArr::kStereo || unsafe { *outputs } != SpeakerArr::kStereo {
            return kResultFalse;
        }
        kResultTrue
    }

    unsafe fn getBusArrangement(
        &self,
        dir: BusDirection,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        if arr.is_null() || index != 0 {
            return kInvalidArgument;
        }
        if matches!(
            dir as BusDirections,
            BusDirections_::kInput | BusDirections_::kOutput
        ) {
            unsafe {
                *arr = SpeakerArr::kStereo;
            }
            kResultOk
        } else {
            kInvalidArgument
        }
    }

    unsafe fn canProcessSampleSize(&self, symbolic_sample_size: i32) -> tresult {
        match symbolic_sample_size as SymbolicSampleSizes {
            SymbolicSampleSizes_::kSample32 => kResultOk,
            SymbolicSampleSizes_::kSample64 => kNotImplemented,
            _ => kInvalidArgument,
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if let Some(setup) = unsafe { setup.as_ref() } {
            // The host calls setupProcessing outside the realtime process path.
            unsafe {
                (&mut *self.analyzer.get()).set_sample_rate_hz(setup.sampleRate as f32);
            }
        }
        kResultOk
    }

    unsafe fn setProcessing(&self, state: TBool) -> tresult {
        self.shared.set_active(state != 0);
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let Some(data) = (unsafe { data.as_ref() }) else {
            return kInvalidArgument;
        };
        if data.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as i32 {
            return process_ok();
        }

        let Some(buffers) = (unsafe { stereo_f32_buffers(data) }) else {
            return process_ok();
        };
        let block_start_sample = process_context_sample_position(data);
        let published = {
            let analyzer = unsafe { &mut *self.analyzer.get() };
            let requested = self.should_analyze();
            let was_requested = unsafe { &mut *self.analysis_requested.get() };
            if requested != *was_requested {
                analyzer.reset();
            }
            *was_requested = requested;
            if requested
                && analyzer.process_stereo_block_at(
                    buffers.input_left,
                    buffers.input_right,
                    block_start_sample,
                )
            {
                analyzer.latest_frame()
            } else {
                None
            }
        };
        if let Some(frame) = published {
            self.shared.publish_frame(&frame);
        }

        for sample_index in 0..buffers.num_samples {
            buffers.output_left[sample_index] = buffers.input_left[sample_index];
            buffers.output_right[sample_index] = buffers.input_right[sample_index];
        }
        process_ok()
    }

    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

impl IProcessContextRequirementsTrait for ChromascopeVst3Processor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        IProcessContextRequirements_::Flags_::kNeedContinousTimeSamples
    }
}

/// Read the host's common sample timeline for one process block.
///
/// Continuous time is preferred because it does not jump at loop boundaries.
/// The VST3 contract defines `projectTimeSamples` as always valid, so it is a
/// safe fallback when the optional continuous-time validity flag is absent.
/// A missing process context is explicitly untimed.
fn process_context_sample_position(data: &ProcessData) -> Option<i64> {
    let context = unsafe { data.processContext.as_ref() }?;
    if context.state & ProcessContext_::StatesAndFlags_::kContTimeValid != 0 {
        Some(context.continousTimeSamples)
    } else {
        Some(context.projectTimeSamples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_processor_only_requests_fft_work_when_a_viewer_is_interested() {
        let shared = Arc::new(ChromascopeShared::new(DeviceKind::Companion));
        let processor = ChromascopeVst3Processor::new(DeviceKind::Companion, shared.clone());

        assert!(!processor.should_analyze());
        let id = shared
            .companion_id()
            .expect("test companion should have a registry id");
        assert!(crate::registry::set_companion_analysis_interest(id, true));
        assert!(processor.should_analyze());
        assert!(crate::registry::set_companion_analysis_interest(id, false));
        assert!(!processor.should_analyze());
    }

    #[test]
    fn processor_requests_the_continuous_sample_timeline() {
        assert_eq!(
            unsafe {
                ChromascopeVst3Processor::new(
                    DeviceKind::Viewer,
                    Arc::new(ChromascopeShared::new(DeviceKind::Viewer)),
                )
                .getProcessContextRequirements()
            },
            IProcessContextRequirements_::Flags_::kNeedContinousTimeSamples
        );
    }

    #[test]
    fn process_context_position_prefers_continuous_and_falls_back_to_project_time() {
        let mut project_context: ProcessContext = unsafe { std::mem::zeroed() };
        project_context.projectTimeSamples = 1_000;
        project_context.continousTimeSamples = 2_000;
        let mut project_data: ProcessData = unsafe { std::mem::zeroed() };
        project_data.processContext = &mut project_context;
        assert_eq!(process_context_sample_position(&project_data), Some(1_000));

        let mut continuous_context: ProcessContext = unsafe { std::mem::zeroed() };
        continuous_context.projectTimeSamples = 1_000;
        continuous_context.continousTimeSamples = 2_000;
        continuous_context.state = ProcessContext_::StatesAndFlags_::kContTimeValid;
        let mut continuous_data: ProcessData = unsafe { std::mem::zeroed() };
        continuous_data.processContext = &mut continuous_context;
        assert_eq!(
            process_context_sample_position(&continuous_data),
            Some(2_000)
        );

        let empty_data: ProcessData = unsafe { std::mem::zeroed() };
        assert_eq!(process_context_sample_position(&empty_data), None);
    }
}
