//! VST3 edit controllers and the Toybox-hosted viewer editor.

use std::ffi::CStr;
use std::ptr;
use std::sync::Arc;

use toybox::vst3::prelude::Steinberg::Vst::ChannelContext;
use toybox::vst3::prelude::Steinberg::*;
use toybox::vst3::prelude::*;

use crate::constants::{Rgb, WINDOW_HEIGHT, WINDOW_WIDTH};
#[cfg(target_os = "macos")]
use crate::gui::preferred_window_size;
#[cfg(target_os = "windows")]
use crate::gui::preferred_window_size;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use crate::gui::{ChromascopeGui, preferred_window_size};
use crate::instance_registry::{SharedRole, release_shared_for_role};
use crate::shared::{ChromascopeShared, DeviceKind};

/// VST3 controller paired with one viewer or companion processor.
pub(super) struct ChromascopeVst3Controller {
    kind: DeviceKind,
    shared: Arc<ChromascopeShared>,
}

impl ChromascopeVst3Controller {
    /// Create a controller for one already acquired shared runtime.
    pub(super) fn new(kind: DeviceKind, shared: Arc<ChromascopeShared>) -> Self {
        Self { kind, shared }
    }
}

impl Drop for ChromascopeVst3Controller {
    fn drop(&mut self) {
        release_shared_for_role(&self.shared, SharedRole::Controller);
    }
}

impl Class for ChromascopeVst3Controller {
    type Interfaces = (IEditController, ChannelContext::IInfoListener);
}

impl IPluginBaseTrait for ChromascopeVst3Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for ChromascopeVst3Controller {
    unsafe fn setComponentState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> int32 {
        0
    }

    unsafe fn getParameterInfo(&self, _index: int32, _info: *mut ParameterInfo) -> tresult {
        kInvalidArgument
    }

    unsafe fn getParamStringByValue(
        &self,
        _id: ParamID,
        _value_normalized: ParamValue,
        _string: *mut String128,
    ) -> tresult {
        kInvalidArgument
    }

    unsafe fn getParamValueByString(
        &self,
        _id: ParamID,
        _string: *mut TChar,
        _value_normalized: *mut ParamValue,
    ) -> tresult {
        kInvalidArgument
    }

    unsafe fn normalizedParamToPlain(
        &self,
        _id: ParamID,
        _value_normalized: ParamValue,
    ) -> ParamValue {
        0.0
    }

    unsafe fn plainParamToNormalized(&self, _id: ParamID, _plain_value: ParamValue) -> ParamValue {
        0.0
    }

    unsafe fn getParamNormalized(&self, _id: ParamID) -> ParamValue {
        0.0
    }

    unsafe fn setParamNormalized(&self, _id: ParamID, _value: ParamValue) -> tresult {
        kInvalidArgument
    }

    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }

    unsafe fn createView(&self, name: FIDString) -> *mut IPlugView {
        if self.kind != DeviceKind::Viewer || name.is_null() {
            return ptr::null_mut();
        }

        let requested = unsafe { CStr::from_ptr(name) };
        let editor = unsafe { CStr::from_ptr(ViewType::kEditor) };
        if requested.to_bytes() != editor.to_bytes() {
            return ptr::null_mut();
        }

        let (width, height) = preferred_window_size();
        let adapter = ChromascopeVst3GuiAdapter::new(self.shared.clone());
        let Some(view) = ComWrapper::new(
            HostedVst3View::new(adapter, width, height).with_size_bounds(
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                WINDOW_WIDTH * 2,
                WINDOW_HEIGHT * 2,
            ),
        )
        .to_com_ptr::<IPlugView>() else {
            return ptr::null_mut();
        };
        ComPtr::into_raw(view)
    }
}

struct ChannelContextUpdate {
    name_present: bool,
    name: Option<String>,
    color_present: bool,
    color: Option<Rgb>,
}

impl ChannelContext::IInfoListenerTrait for ChromascopeVst3Controller {
    unsafe fn setChannelContextInfos(&self, list: *mut IAttributeList) -> tresult {
        if list.is_null() {
            return kInvalidArgument;
        }

        let update = read_channel_context(list);
        if update.name_present {
            self.shared.set_host_name(update.name);
        }
        if update.color_present {
            self.shared.set_host_color(update.color);
        }
        kResultTrue
    }
}

fn read_channel_context(list: *mut IAttributeList) -> ChannelContextUpdate {
    let Some(attributes) = (unsafe { ComRef::from_raw(list) }) else {
        return ChannelContextUpdate {
            name_present: false,
            name: None,
            color_present: false,
            color: None,
        };
    };

    let mut name_buffer = [0 as TChar; 128];
    let name_present = unsafe {
        attributes.getString(
            ChannelContext::kChannelNameKey,
            name_buffer.as_mut_ptr(),
            std::mem::size_of_val(&name_buffer) as uint32,
        ) == kResultTrue
    };
    let name = name_present
        .then(|| decode_channel_name(&name_buffer))
        .flatten();

    let mut color = 0_i64;
    let color_present =
        unsafe { attributes.getInt(ChannelContext::kChannelColorKey, &mut color) == kResultTrue };
    let color = color_present.then(|| decode_channel_color(color));

    ChannelContextUpdate {
        name_present,
        name,
        color_present,
        color,
    }
}

fn decode_channel_name(buffer: &[TChar]) -> Option<String> {
    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    let name = String::from_utf16_lossy(&buffer[..length]);
    (!name.trim().is_empty()).then_some(name)
}

fn decode_channel_color(color: int64) -> Rgb {
    let color = color as u64 as u32;
    Rgb::new((color >> 16) as u8, (color >> 8) as u8, color as u8)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct ChromascopeVst3GuiAdapter {
    gui: toybox::radiant_gui::RadiantHostedGui,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct ChromascopeVst3GuiAdapter {
    shared: Arc<ChromascopeShared>,
    gui: ChromascopeGui,
}

impl ChromascopeVst3GuiAdapter {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn new(shared: Arc<ChromascopeShared>) -> Self {
        Self {
            gui: crate::radiant_editor::new_gui(shared),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn new(shared: Arc<ChromascopeShared>) -> Self {
        Self {
            shared,
            gui: ChromascopeGui::default(),
        }
    }
}

impl Vst3HostedGui for ChromascopeVst3GuiAdapter {
    fn set_parent_raw(&mut self, parent: toybox::raw_window_handle::RawWindowHandle) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.gui.set_parent(parent);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        self.gui.set_parent_raw(parent);
    }

    fn open(&mut self) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return self.gui.open();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        self.gui.open(self.shared.clone()).is_ok()
    }

    fn close(&mut self) {
        self.gui.close();
    }

    fn last_size(&self) -> Option<(u32, u32)> {
        self.gui.last_size()
    }

    fn request_resize(&self, width: u32, height: u32) {
        self.gui.request_resize(width, height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, c_void};
    use std::ptr;
    use toybox::vst3::prelude::Steinberg::Vst::ChannelContext::IInfoListenerTrait;

    fn attribute_id_matches(
        id: *const std::ffi::c_char,
        expected: *const std::ffi::c_char,
    ) -> bool {
        if id.is_null() || expected.is_null() {
            return false;
        }
        unsafe { CStr::from_ptr(id) == CStr::from_ptr(expected) }
    }

    struct TestAttributes {
        name: Vec<TChar>,
        color: int64,
    }

    impl Class for TestAttributes {
        type Interfaces = (IAttributeList,);
    }

    impl IAttributeListTrait for TestAttributes {
        unsafe fn setInt(&self, _id: *const std::ffi::c_char, _value: int64) -> tresult {
            kResultFalse
        }

        unsafe fn getInt(&self, id: *const std::ffi::c_char, value: *mut int64) -> tresult {
            if !attribute_id_matches(id, ChannelContext::kChannelColorKey) || value.is_null() {
                return kResultFalse;
            }
            unsafe { *value = self.color };
            kResultTrue
        }

        unsafe fn setFloat(&self, _id: *const std::ffi::c_char, _value: f64) -> tresult {
            kResultFalse
        }

        unsafe fn getFloat(&self, _id: *const std::ffi::c_char, _value: *mut f64) -> tresult {
            kResultFalse
        }

        unsafe fn setString(&self, _id: *const std::ffi::c_char, _string: *const TChar) -> tresult {
            kResultFalse
        }

        unsafe fn getString(
            &self,
            id: *const std::ffi::c_char,
            string: *mut TChar,
            size_in_bytes: uint32,
        ) -> tresult {
            let capacity = size_in_bytes as usize / std::mem::size_of::<TChar>();
            if !attribute_id_matches(id, ChannelContext::kChannelNameKey)
                || string.is_null()
                || capacity == 0
            {
                return kResultFalse;
            }
            let length = self.name.len().min(capacity.saturating_sub(1));
            unsafe {
                ptr::copy_nonoverlapping(self.name.as_ptr(), string, length);
                *string.add(length) = 0;
            }
            kResultTrue
        }

        unsafe fn setBinary(
            &self,
            _id: *const std::ffi::c_char,
            _data: *const c_void,
            _size_in_bytes: uint32,
        ) -> tresult {
            kResultFalse
        }

        unsafe fn getBinary(
            &self,
            _id: *const std::ffi::c_char,
            _data: *mut *const c_void,
            _size_in_bytes: *mut uint32,
        ) -> tresult {
            kResultFalse
        }
    }

    #[test]
    fn channel_context_decoding_accepts_utf16_name_and_argb_color() {
        let mut name = [0 as TChar; 128];
        let encoded: Vec<TChar> = "Bass Bus".encode_utf16().collect();
        name[..encoded.len()].copy_from_slice(&encoded);
        assert_eq!(decode_channel_name(&name).as_deref(), Some("Bass Bus"));
        assert_eq!(
            decode_channel_color(0xCC_11_82_EE),
            Rgb::new(0x11, 0x82, 0xEE)
        );
    }

    #[test]
    fn empty_channel_context_name_is_not_used_as_a_visible_label() {
        assert_eq!(decode_channel_name(&[0 as TChar; 128]), None);
        assert_eq!(decode_channel_name(&[32 as TChar; 2]), None);
    }

    #[test]
    fn controller_callback_updates_the_registered_companion_identity() {
        let shared = Arc::new(ChromascopeShared::new(DeviceKind::Companion));
        let id = shared
            .companion_id()
            .expect("companion runtime should register a source");
        let controller = ChromascopeVst3Controller::new(DeviceKind::Companion, shared);
        let attributes = ComWrapper::new(TestAttributes {
            name: "Bass Bus".encode_utf16().collect(),
            color: 0xCC_11_82_EE,
        });
        let list = attributes
            .as_com_ref::<IAttributeList>()
            .expect("test attributes should expose IAttributeList")
            .as_ptr();

        assert_eq!(
            unsafe { controller.setChannelContextInfos(list) },
            kResultTrue
        );
        let source = crate::registry::snapshot_companions()
            .into_iter()
            .find(|source| source.id == id)
            .expect("updated source should remain discoverable");
        assert_eq!(source.name, "Bass Bus");
        assert_eq!(source.color, Rgb::new(0x11, 0x82, 0xEE));
    }
}
