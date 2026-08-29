//! VST3 factory exposing both Chromascope device classes in one bundle.

use std::ffi::c_void;

use toybox::vst3::prelude::Steinberg::*;
use toybox::vst3::prelude::*;

use crate::constants::{COMPANION_NAME, PLUGIN_NAME};
use crate::instance_registry::{SharedRole, acquire_shared_for_role};
use crate::shared::DeviceKind;

use super::controller::ChromascopeVst3Controller;
use super::processor::ChromascopeVst3Processor;
use super::{
    COMPANION_CONTROLLER_CID, COMPANION_PROCESSOR_CID, VIEWER_CONTROLLER_CID, VIEWER_PROCESSOR_CID,
};

#[derive(Default)]
pub(super) struct ChromascopeVst3Factory;

impl Class for ChromascopeVst3Factory {
    type Interfaces = (IPluginFactory,);
}

impl IPluginBaseTrait for ChromascopeVst3Factory {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IPluginFactoryTrait for ChromascopeVst3Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        copy_cstring("PORTALSURFER", &mut info.vendor);
        copy_cstring("https://github.com/PORTALSURFER/audiodev", &mut info.url);
        copy_cstring("support@localhost", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as i32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        4
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        match index {
            0 => {
                write_class_info_many(
                    info,
                    VIEWER_PROCESSOR_CID,
                    CATEGORY_AUDIO_MODULE_CLASS,
                    PLUGIN_NAME,
                );
                kResultOk
            }
            1 => {
                write_class_info_many(
                    info,
                    VIEWER_CONTROLLER_CID,
                    CATEGORY_COMPONENT_CONTROLLER_CLASS,
                    PLUGIN_NAME,
                );
                kResultOk
            }
            2 => {
                write_class_info_many(
                    info,
                    COMPANION_PROCESSOR_CID,
                    CATEGORY_AUDIO_MODULE_CLASS,
                    COMPANION_NAME,
                );
                kResultOk
            }
            3 => {
                write_class_info_many(
                    info,
                    COMPANION_CONTROLLER_CID,
                    CATEGORY_COMPONENT_CONTROLLER_CLASS,
                    COMPANION_NAME,
                );
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }

        let class_id = unsafe { *(cid as *const TUID) };
        let instance = match class_id {
            VIEWER_PROCESSOR_CID => ComWrapper::new(ChromascopeVst3Processor::new(
                DeviceKind::Viewer,
                acquire_shared_for_role(DeviceKind::Viewer, SharedRole::Processor),
            ))
            .to_com_ptr::<FUnknown>(),
            VIEWER_CONTROLLER_CID => ComWrapper::new(ChromascopeVst3Controller::new(
                DeviceKind::Viewer,
                acquire_shared_for_role(DeviceKind::Viewer, SharedRole::Controller),
            ))
            .to_com_ptr::<FUnknown>(),
            COMPANION_PROCESSOR_CID => ComWrapper::new(ChromascopeVst3Processor::new(
                DeviceKind::Companion,
                acquire_shared_for_role(DeviceKind::Companion, SharedRole::Processor),
            ))
            .to_com_ptr::<FUnknown>(),
            COMPANION_CONTROLLER_CID => ComWrapper::new(ChromascopeVst3Controller::new(
                DeviceKind::Companion,
                acquire_shared_for_role(DeviceKind::Companion, SharedRole::Controller),
            ))
            .to_com_ptr::<FUnknown>(),
            _ => None,
        };
        let Some(instance) = instance else {
            return kInvalidArgument;
        };
        let ptr = instance.as_ptr();
        unsafe { ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj) }
    }
}

toybox::vst3_plugin_entry!(ChromascopeVst3Factory);
