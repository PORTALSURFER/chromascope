//! Non-realtime pairing of VST3 processors with their controllers.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::shared::{ChromascopeShared, DeviceKind};

/// One factory-side role claimed by a VST3 component.
#[derive(Clone, Copy)]
pub(crate) enum SharedRole {
    /// Audio processor role.
    Processor,
    /// Edit controller role.
    Controller,
}

struct InstanceEntry {
    kind: DeviceKind,
    shared: Weak<ChromascopeShared>,
    processor_claimed: bool,
    controller_claimed: bool,
}

fn instance_registry() -> &'static Mutex<Vec<InstanceEntry>> {
    static REGISTRY: OnceLock<Mutex<Vec<InstanceEntry>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Acquire a shared runtime for one VST3 class role.
pub(crate) fn acquire_shared_for_role(
    kind: DeviceKind,
    role: SharedRole,
) -> Arc<ChromascopeShared> {
    let Ok(mut registry) = instance_registry().lock() else {
        return Arc::new(ChromascopeShared::new(kind));
    };
    registry.retain(|entry| entry.shared.strong_count() > 0);

    for entry in registry.iter_mut() {
        if entry.kind != kind {
            continue;
        }
        let Some(shared) = entry.shared.upgrade() else {
            continue;
        };
        match role {
            SharedRole::Processor if !entry.processor_claimed => {
                entry.processor_claimed = true;
                return shared;
            }
            SharedRole::Controller if !entry.controller_claimed => {
                entry.controller_claimed = true;
                return shared;
            }
            _ => {}
        }
    }

    let shared = Arc::new(ChromascopeShared::new(kind));
    registry.push(InstanceEntry {
        kind,
        shared: Arc::downgrade(&shared),
        processor_claimed: matches!(role, SharedRole::Processor),
        controller_claimed: matches!(role, SharedRole::Controller),
    });
    shared
}

/// Release one non-realtime processor/controller role claim.
pub(crate) fn release_shared_for_role(shared: &Arc<ChromascopeShared>, role: SharedRole) {
    let Ok(mut registry) = instance_registry().lock() else {
        return;
    };
    registry.retain(|entry| entry.shared.strong_count() > 0);
    for entry in registry.iter_mut() {
        let Some(candidate) = entry.shared.upgrade() else {
            continue;
        };
        if !Arc::ptr_eq(&candidate, shared) {
            continue;
        }
        match role {
            SharedRole::Processor => entry.processor_claimed = false,
            SharedRole::Controller => entry.controller_claimed = false,
        }
    }
}
