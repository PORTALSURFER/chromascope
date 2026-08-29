//! Toybox declarative viewer UI and VST3 host-window adapter.

use std::sync::Arc;

use toybox::clack_extensions::gui::{GuiSize, Window};
use toybox::clack_plugin::plugin::PluginError;
use toybox::clap::gui::{GuiHostWindow, GuiOpenRequest, InputState};
use toybox::gui::declarative::{
    Node, Slot, SlotParams, UiAction, UiSpec, column_slots, fill_slot, panel, root_frame_sized,
    row_slots, scroll_view, surface, textbox, toggle, weighted_slot,
};
use toybox::gui::{Color, Size};
use toybox::raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

use crate::alignment::common_target_sample;
use crate::analysis::SpectrumFrame;
use crate::constants::{
    ALIGNMENT_MAX_SKEW_SAMPLES, MAIN_SPECTRUM_COLOR, MAX_COMPANIONS, PLUGIN_NAME, SPECTRUM_HEADER,
    WINDOW_HEIGHT, WINDOW_WIDTH,
};
use crate::registry::{CompanionSourceSnapshot, snapshot_companions_at};
use crate::render::build_spectrum_surface_commands;
use crate::shared::ChromascopeShared;
use crate::visual_system::{PUMP_PALETTE, pump_aligned_theme_tokens, to_gui_color};

/// Stable key for the root frame.
pub const ROOT_KEY: &str = "chromascope-root";
/// Stable key for the graph surface.
pub const SPECTRUM_SURFACE_KEY: &str = "chromascope-spectrum";
/// Stable key for the source-list region.
pub const SOURCE_LIST_KEY: &str = "chromascope-sources";
const GRAPH_KEY: &str = "chromascope-graph";
const SOURCE_PANEL_KEY: &str = "chromascope-source-panel";
const SOURCE_ROW_PREFIX: &str = "chromascope-source-";
const GRAPH_WEIGHT: u16 = 4;
const SOURCE_PANEL_WEIGHT: u16 = 1;

/// Return the preferred default editor size.
pub fn preferred_window_size() -> (u32, u32) {
    (WINDOW_WIDTH, WINDOW_HEIGHT)
}

/// Host-window wrapper for the Chromascope viewer editor.
#[derive(Default)]
pub struct ChromascopeGui {
    window: GuiHostWindow,
}

impl ChromascopeGui {
    /// Attach the raw host parent window.
    pub fn set_parent_raw(&mut self, parent: RawWindowHandle) {
        self.window.set_parent(parent);
    }

    /// Attach a CLAP-compatible host parent window.
    pub fn set_parent(&mut self, window: Window<'_>) {
        self.set_parent_raw(window.raw_window_handle());
    }

    /// Open the viewer editor for one paired runtime.
    pub fn open(&mut self, shared: Arc<ChromascopeShared>) -> Result<(), PluginError> {
        self.window
            .set_aspect_ratio(Some(WINDOW_WIDTH as f32 / WINDOW_HEIGHT as f32));
        self.window
            .open_parented_with(GuiOpenRequest::<GuiState, _, _, _>::new(
                PLUGIN_NAME.to_string(),
                (WINDOW_WIDTH, WINDOW_HEIGHT),
                GuiState::new(shared),
                Box::new(|_state: &mut GuiState| {}),
                Box::new(|input: &InputState, state: &GuiState| state.build_ui(input)),
                Box::new(|state: &mut GuiState, action: UiAction| state.reduce_action(action)),
            ))
    }

    /// Request a host resize for the viewer editor.
    pub fn request_resize(&self, width: u32, height: u32) {
        self.window.request_resize(width, height);
    }

    /// Hide the viewer editor.
    pub fn close(&mut self) {
        self.window.hide();
    }

    /// Return the last logical editor size reported by the host.
    pub fn last_size(&self) -> Option<(u32, u32)> {
        self.window.last_size()
    }

    /// Clamp a host-provided size against the viewer's minimum size.
    pub fn adjust_host_size(&self, size: GuiSize) -> Option<GuiSize> {
        self.window.adjust_host_size(size).map(|size| GuiSize {
            width: size.width.max(WINDOW_WIDTH),
            height: size.height.max(WINDOW_HEIGHT),
        })
    }

    /// Apply a host-provided size to the native editor window.
    pub fn apply_host_size(&self, size: GuiSize) {
        self.window.apply_host_size(GuiSize {
            width: size.width.max(WINDOW_WIDTH),
            height: size.height.max(WINDOW_HEIGHT),
        });
    }
}

/// Build a complete viewer UI from one immutable audio/UI snapshot.
pub fn build_ui_spec(
    window_size: Size,
    main_frame: Option<SpectrumFrame>,
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
) -> UiSpec {
    let size = Size {
        width: window_size.width.max(WINDOW_WIDTH),
        height: window_size.height.max(WINDOW_HEIGHT),
    };
    let graph_width = size.width.saturating_mul(GRAPH_WEIGHT as u32)
        / (GRAPH_WEIGHT + SOURCE_PANEL_WEIGHT) as u32;
    let graph_commands = build_spectrum_surface_commands(
        main_frame,
        companions,
        selected_ids,
        Size {
            width: graph_width,
            height: size.height.saturating_sub(16),
        },
    );
    let graph = panel(
        GRAPH_KEY,
        column_slots(vec![
            weighted_slot(
                textbox(SPECTRUM_HEADER).text_color(to_gui_color(PUMP_PALETTE.text_primary)),
                1,
            ),
            weighted_slot(
                surface(
                    SPECTRUM_SURFACE_KEY,
                    Size {
                        width: graph_width,
                        height: size.height.saturating_sub(16),
                    },
                    graph_commands,
                )
                .fill(),
                20,
            ),
        ]),
    )
    .background(to_gui_color(PUMP_PALETTE.surface))
    .outline(to_gui_color(PUMP_PALETTE.border));

    let source_panel = panel(
        SOURCE_PANEL_KEY,
        column_slots(vec![
            weighted_slot(
                row_slots(vec![weighted_slot(
                    textbox("MAIN INPUT").text_color(to_color(MAIN_SPECTRUM_COLOR)),
                    100,
                )]),
                1,
            ),
            weighted_slot(
                textbox("COMPANIONS  •  SCROLL")
                    .text_color(to_gui_color(PUMP_PALETTE.text_primary)),
                1,
            ),
            weighted_slot(
                scroll_view(column_slots(source_rows(companions, selected_ids))).fill(),
                20,
            ),
        ]),
    )
    .background(to_gui_color(PUMP_PALETTE.surface))
    .outline(to_gui_color(PUMP_PALETTE.border))
    .title(format!(
        "SOURCES  {}/{}",
        selected_ids.len(),
        MAX_COMPANIONS
    ));

    let content = row_slots(vec![
        weighted_slot(graph, GRAPH_WEIGHT),
        weighted_slot(source_panel, SOURCE_PANEL_WEIGHT),
    ]);
    UiSpec::new(
        root_frame_sized(ROOT_KEY, content, size)
            .title(PLUGIN_NAME)
            .padding(10)
            .tokens(pump_aligned_theme_tokens()),
    )
}

/// Build the initial empty viewer UI.
pub fn build_spec() -> UiSpec {
    build_ui_spec(
        Size {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
        None,
        &[],
        &[],
    )
}

fn source_rows(
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
) -> Vec<toybox::gui::declarative::Slot> {
    if companions.is_empty() {
        return vec![fill_slot(
            textbox("NO COMPANIONS REGISTERED").text_color(to_gui_color(PUMP_PALETTE.text_muted)),
        )];
    }

    companions
        .iter()
        // Each source row is intrinsically sized so the scroll view owns a
        // compact content height.  Weighted rows would divide the viewport
        // among every registered companion, making a handful of rows grow to
        // fill the panel and obscuring their labels/markers.
        .map(|source| {
            Slot::with_params(
                source_row(source, selected_ids.contains(&source.id)),
                SlotParams::intrinsic(),
            )
        })
        .collect()
}

fn source_row(source: &CompanionSourceSnapshot, selected: bool) -> Node {
    let status = if !source.active {
        "INACTIVE"
    } else if !source.analysis_requested {
        "IDLE"
    } else if source.frame.is_some() {
        "LIVE"
    } else {
        "WARMING"
    };
    row_slots(vec![
        weighted_slot(
            textbox("●")
                .text_color(to_color(source.color))
                .text_align_center(),
            14,
        ),
        weighted_slot(textbox(source.name.clone()), 44),
        weighted_slot(
            textbox(status).text_color(status_color(
                source.active,
                source.analysis_requested,
                source.frame.is_some(),
            )),
            26,
        ),
        weighted_slot(
            toggle(source_key(source.id), selected)
                .control_size(Size {
                    width: 30,
                    height: 20,
                })
                .disabled(!source.active),
            16,
        ),
    ])
}

fn source_key(id: u64) -> String {
    format!("{SOURCE_ROW_PREFIX}{id}")
}

fn source_id_from_key(key: &str) -> Option<u64> {
    key.strip_prefix(SOURCE_ROW_PREFIX)?.parse().ok()
}

fn status_color(active: bool, requested: bool, ready: bool) -> Color {
    if !active || !requested {
        to_gui_color(PUMP_PALETTE.text_muted)
    } else if ready {
        to_gui_color(PUMP_PALETTE.accent_secondary)
    } else {
        to_gui_color(PUMP_PALETTE.warning)
    }
}

fn to_color(rgb: crate::constants::Rgb) -> Color {
    Color::rgb(rgb.red, rgb.green, rgb.blue)
}

struct GuiState {
    shared: Arc<ChromascopeShared>,
    selected_ids: Vec<u64>,
}

impl GuiState {
    fn new(shared: Arc<ChromascopeShared>) -> Self {
        Self {
            shared,
            selected_ids: Vec::new(),
        }
    }

    fn build_ui(&self, input: &InputState) -> UiSpec {
        let latest_companions = crate::registry::snapshot_companions();
        let latest_main = self.shared.spectrum.read();
        let target = common_target_sample(latest_main, &latest_companions, &self.selected_ids);
        let main_frame = target
            .and_then(|target| {
                self.shared
                    .spectrum
                    .read_nearest(target, ALIGNMENT_MAX_SKEW_SAMPLES)
            })
            .or(latest_main);
        let companions = target
            .map(|target| snapshot_companions_at(Some(target)))
            .unwrap_or(latest_companions);
        let selected_ids: Vec<u64> = self
            .selected_ids
            .iter()
            .copied()
            .filter(|id| companions.iter().any(|source| source.id == *id))
            .collect();
        build_ui_spec(input.window_size, main_frame, &companions, &selected_ids)
    }

    fn reduce_action(&mut self, action: UiAction) {
        self.prune_stale_selections();
        if let UiAction::ToggleChanged { key, value } = action
            && let Some(id) = source_id_from_key(&key)
        {
            if value {
                if !self.selected_ids.contains(&id) && self.selected_ids.len() < MAX_COMPANIONS {
                    crate::registry::set_companion_analysis_interest(id, true);
                    self.selected_ids.push(id);
                }
            } else {
                if self.selected_ids.contains(&id) {
                    crate::registry::set_companion_analysis_interest(id, false);
                    self.selected_ids.retain(|selected| *selected != id);
                }
            }
        }
    }

    fn prune_stale_selections(&mut self) {
        let companions = crate::registry::snapshot_companions();
        let mut index = 0;
        while index < self.selected_ids.len() {
            if companions
                .iter()
                .any(|source| source.id == self.selected_ids[index])
            {
                index += 1;
            } else {
                let id = self.selected_ids.remove(index);
                crate::registry::set_companion_analysis_interest(id, false);
            }
        }
    }
}

impl Drop for GuiState {
    fn drop(&mut self) {
        for id in self.selected_ids.drain(..) {
            crate::registry::set_companion_analysis_interest(id, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::MAX_BANDS;
    use toybox::gui::declarative::measure_checked;

    fn source(id: u64, active: bool) -> CompanionSourceSnapshot {
        CompanionSourceSnapshot {
            id,
            name: format!("COMPANION {id}"),
            color: crate::registry::color_from_seed(id),
            active,
            analysis_requested: active,
            frame: active.then(|| SpectrumFrame {
                sequence: 1,
                sample_position: None,
                values: [-18.0; MAX_BANDS],
            }),
        }
    }

    #[test]
    fn emitted_ui_spec_passes_strict_slot_validation() {
        let companions = [source(7, true), source(8, false)];
        let spec = build_ui_spec(
            Size {
                width: WINDOW_WIDTH,
                height: WINDOW_HEIGHT,
            },
            None,
            &companions,
            &[7],
        );
        measure_checked(&spec).expect("Chromascope UI must obey Toybox slot grammar");
    }

    #[test]
    fn multiple_companion_selection_is_reduced_without_host_selection_apis() {
        let first = crate::registry::register_companion().expect("first source");
        let second = crate::registry::register_companion().expect("second source");
        let first_id = first.id();
        let second_id = second.id();
        let shared = Arc::new(ChromascopeShared::new(crate::shared::DeviceKind::Viewer));
        let mut state = GuiState::new(shared);
        state.reduce_action(UiAction::ToggleChanged {
            key: source_key(first_id),
            value: true,
        });
        state.reduce_action(UiAction::ToggleChanged {
            key: source_key(second_id),
            value: true,
        });
        assert_eq!(state.selected_ids, vec![first_id, second_id]);
        state.reduce_action(UiAction::ToggleChanged {
            key: source_key(first_id),
            value: false,
        });
        assert_eq!(state.selected_ids, vec![second_id]);
    }

    #[test]
    fn inactive_source_is_shown_but_cannot_be_selected_by_the_widget() {
        let node = source_row(&source(4, false), false);
        assert!(matches!(node, Node::Grid(_)));
        assert_eq!(source_id_from_key(&source_key(4)), Some(4));
    }

    #[test]
    fn preferred_editor_size_is_a_wide_scope_ratio() {
        let (width, height) = preferred_window_size();
        assert_eq!(width * 4, height * 9);
        assert!(width >= height * 2);
    }
}

#[cfg(all(test, feature = "screenshot-test"))]
mod screenshot_tests {
    use super::*;
    use toybox::gui::screenshot_harness;

    #[test]
    fn screenshot_renders_initial_ui() {
        screenshot_harness::capture_initial_ui_screenshots_if_enabled(
            env!("CARGO_PKG_NAME"),
            Size {
                width: WINDOW_WIDTH,
                height: WINDOW_HEIGHT,
            },
            |_input| build_spec(),
        )
        .expect("failed to capture headless screenshots");
    }

    #[test]
    fn screenshot_size_matrix_remains_wide() {
        for size in screenshot_harness::default_screenshot_sizes(Size {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }) {
            assert!(size.width >= size.height * 2);
        }
    }

    #[test]
    fn screenshot_renders_populated_smooth_trace_ui() {
        let companions = [
            CompanionSourceSnapshot {
                id: 101,
                name: "KICK BUS".to_string(),
                color: crate::registry::color_from_seed(101),
                active: true,
                analysis_requested: true,
                frame: Some(SpectrumFrame {
                    sequence: 1,
                    sample_position: Some(8_192),
                    values: [-34.0; crate::constants::MAX_BANDS],
                }),
            },
            CompanionSourceSnapshot {
                id: 102,
                name: "VOCAL BUS".to_string(),
                color: crate::registry::color_from_seed(102),
                active: true,
                analysis_requested: true,
                frame: Some(SpectrumFrame {
                    sequence: 2,
                    sample_position: Some(8_192),
                    values: [-52.0; crate::constants::MAX_BANDS],
                }),
            },
        ];
        let main = SpectrumFrame {
            sequence: 3,
            sample_position: Some(8_192),
            values: [-18.0; crate::constants::MAX_BANDS],
        };
        let plugin = format!("{}-trace", env!("CARGO_PKG_NAME"));

        screenshot_harness::capture_initial_ui_screenshots_if_enabled(
            &plugin,
            Size {
                width: WINDOW_WIDTH,
                height: WINDOW_HEIGHT,
            },
            |input| build_ui_spec(input.window_size, Some(main), &companions, &[101, 102]),
        )
        .expect("failed to capture populated smooth-trace screenshots");
    }
}
