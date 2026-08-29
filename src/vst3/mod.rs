//! Toybox VST3 factory, processor, and controller implementations.

#![allow(clippy::missing_docs_in_private_items)]

use toybox::vst3::prelude::Steinberg::*;
use toybox::vst3::prelude::*;

mod controller;
mod factory;
mod processor;

pub(super) const VIEWER_PROCESSOR_CID: TUID =
    uid(0x6D9E_4F31, 0xA2C7_4B10, 0x8F63_19D5, 0xB70E_24AC);
pub(super) const VIEWER_CONTROLLER_CID: TUID =
    uid(0xA81C_53E7, 0x4D20_9B6A, 0xF1B8_30C2, 0x7E45_D96F);
pub(super) const COMPANION_PROCESSOR_CID: TUID =
    uid(0xC46A_8D12, 0xE903_57BF, 0x2A71_F0C8, 0x9B35_64DE);
pub(super) const COMPANION_CONTROLLER_CID: TUID =
    uid(0xF237_BA09, 0x6C54_18E2, 0xD80F_4391, 0x5EAC_72B6);

#[cfg(target_os = "windows")]
pub(super) const fn vst3_bus_flag(flag: i32) -> u32 {
    flag as u32
}

#[cfg(not(target_os = "windows"))]
pub(super) const fn vst3_bus_flag(flag: u32) -> u32 {
    flag
}
