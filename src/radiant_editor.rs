//! Radiant-backed macOS VST3 editor for the Chromascope viewer.
//!
//! The legacy Toybox Patchbay host is intentionally retained for the portable
//! declarative preview, but its native host window only accepts Win32 parents
//! in this Toybox revision. macOS hosts provide an AppKit `NSView`; this editor
//! uses Toybox's Radiant VST3 bridge for that native path.

use std::sync::Arc;
use std::time::Instant;

use radiant::gui::types::{Point, Rect, Rgba8, Vector2};
use radiant::runtime::{
    Event, PaintBrush, PaintClipEnd, PaintClipStart, PaintFillPath, PaintFillRect, PaintFillRule,
    PaintPath, PaintPathCommand, PaintPrimitive, PaintStrokePolyline, PaintStrokeRect, PaintText,
    PaintTextAlign, PaintTextRun, SurfacePaintPlan,
};
use radiant::widgets::{PointerButton, TextWrap, WidgetKey};

use crate::alignment::common_target_sample;
use crate::analysis::SpectrumFrame;
use crate::constants::{
    ALIGNMENT_MAX_SKEW_SAMPLES, COMPANION_ACTIVITY_BLINK_PERIOD_SECONDS, DISPLAY_TRACE_SAMPLES,
    MAIN_SPECTRUM_COLOR, MAX_BANDS, MAX_COMPANIONS, MAX_FREQUENCY_HZ, MAX_HIGHLIGHTS, MAX_LEVEL_DB,
    MIN_FREQUENCY_HZ, MIN_LEVEL_DB, PLUGIN_NAME, PRESENTATION_FRAME_SECONDS, Rgb, SPECTRUM_HEADER,
    WINDOW_HEIGHT, WINDOW_WIDTH,
};
use crate::highlight::{HighlightToggle, HighlightedSource, color_for, toggle_highlight};
use crate::presentation::PresentationState;
use crate::registry::{
    CompanionActivitySnapshot, CompanionSourceSnapshot, snapshot_companion_activity,
    snapshot_companions_at,
};
use crate::render::{display_trace_band_position, interpolated_band_value};
use crate::shared::ChromascopeShared;
use crate::visual_system::{PUMP_ALIGNED_METRICS, PUMP_ALIGNED_TYPOGRAPHY, PUMP_PALETTE};

const ROOT_WIDGET_ID: u64 = 0x4348_524F_4D41_5301;
const GRAPH_WIDGET_ID: u64 = 0x4348_524F_4D41_5302;
const SOURCE_WIDGET_ID: u64 = 0x4348_524F_4D41_5303;

const BACKGROUND: Rgba8 = Rgba8::new(
    PUMP_PALETTE.canvas.red,
    PUMP_PALETTE.canvas.green,
    PUMP_PALETTE.canvas.blue,
    255,
);
const PANEL_BACKGROUND: Rgba8 = Rgba8::new(
    PUMP_PALETTE.surface.red,
    PUMP_PALETTE.surface.green,
    PUMP_PALETTE.surface.blue,
    255,
);
const BORDER: Rgba8 = Rgba8::new(
    PUMP_PALETTE.border.red,
    PUMP_PALETTE.border.green,
    PUMP_PALETTE.border.blue,
    255,
);
const BORDER_EMPHASIS: Rgba8 = Rgba8::new(
    PUMP_PALETTE.border_emphasis.red,
    PUMP_PALETTE.border_emphasis.green,
    PUMP_PALETTE.border_emphasis.blue,
    255,
);
const MAJOR_GRID: Rgba8 = Rgba8::new(
    PUMP_PALETTE.grid_strong.red,
    PUMP_PALETTE.grid_strong.green,
    PUMP_PALETTE.grid_strong.blue,
    180,
);
const MINOR_GRID: Rgba8 = Rgba8::new(
    PUMP_PALETTE.grid_soft.red,
    PUMP_PALETTE.grid_soft.green,
    PUMP_PALETTE.grid_soft.blue,
    135,
);
const LABEL: Rgba8 = Rgba8::new(
    PUMP_PALETTE.text_muted.red,
    PUMP_PALETTE.text_muted.green,
    PUMP_PALETTE.text_muted.blue,
    255,
);
const SOFT_LABEL: Rgba8 = Rgba8::new(
    PUMP_PALETTE.text_primary.red,
    PUMP_PALETTE.text_primary.green,
    PUMP_PALETTE.text_primary.blue,
    255,
);
const MUTED: Rgba8 = LABEL;
const SELECTED_ROW: Rgba8 = Rgba8::new(
    PUMP_PALETTE.overlay.red,
    PUMP_PALETTE.overlay.green,
    PUMP_PALETTE.overlay.blue,
    255,
);
const INACTIVE: Rgba8 = LABEL;
const WARMING: Rgba8 = Rgba8::new(
    PUMP_PALETTE.warning.red,
    PUMP_PALETTE.warning.green,
    PUMP_PALETTE.warning.blue,
    255,
);
const LIVE: Rgba8 = Rgba8::new(
    PUMP_PALETTE.accent_secondary.red,
    PUMP_PALETTE.accent_secondary.green,
    PUMP_PALETTE.accent_secondary.blue,
    255,
);
const SCROLL_TRACK: Rgba8 = Rgba8::new(
    PUMP_PALETTE.grid_soft.red,
    PUMP_PALETTE.grid_soft.green,
    PUMP_PALETTE.grid_soft.blue,
    150,
);
const SCROLL_THUMB: Rgba8 = Rgba8::new(
    PUMP_PALETTE.text_muted.red,
    PUMP_PALETTE.text_muted.green,
    PUMP_PALETTE.text_muted.blue,
    210,
);
const ACTIVITY_DOT: Rgba8 = Rgba8::new(
    PUMP_PALETTE.danger.red,
    PUMP_PALETTE.danger.green,
    PUMP_PALETTE.danger.blue,
    255,
);

const OUTER_MARGIN: f32 = PUMP_ALIGNED_METRICS.padding;
const PANEL_GAP: f32 = PUMP_ALIGNED_METRICS.gap;
const SOURCE_PANEL_RATIO: f32 = 0.24;
const SOURCE_PANEL_MIN_WIDTH: f32 = 236.0;
const SOURCE_PANEL_MAX_RATIO: f32 = 0.38;
const GRAPH_HEADER_HEIGHT: f32 = 40.8;
const SOURCE_HEADER_HEIGHT: f32 = 34.0;
const SOURCE_LEGEND_HEIGHT: f32 = 34.0;
const SOURCE_LIST_HEADER_HEIGHT: f32 = 27.2;
const SOURCE_ROW_HEIGHT: f32 = PUMP_ALIGNED_METRICS.control_height;
const SOURCE_ROW_GAP: f32 = PUMP_ALIGNED_METRICS.row_gap;
const SOURCE_SCROLL_EPSILON: f32 = 0.01;
const PLOT_PADDING_LEFT: f32 = 44.0;
const PLOT_PADDING_RIGHT: f32 = 12.0;
const PLOT_PADDING_TOP: f32 = 10.0;
const PLOT_PADDING_BOTTOM: f32 = 30.0;
const BRAND_FONT_SIZE: f32 = PUMP_ALIGNED_TYPOGRAPHY.brand.0;
const BODY_FONT_SIZE: f32 = PUMP_ALIGNED_TYPOGRAPHY.body.0;
const VALUE_FONT_SIZE: f32 = PUMP_ALIGNED_TYPOGRAPHY.value.0;
const CONTROL_LABEL_FONT_SIZE: f32 = PUMP_ALIGNED_TYPOGRAPHY.control_label.0;
const META_FONT_SIZE: f32 = PUMP_ALIGNED_TYPOGRAPHY.meta.0;
const MAIN_TRACE_THICKNESS: f32 = 1.7;
const COMPANION_TRACE_THICKNESS: f32 = 1.275;

#[derive(Clone, Copy)]
struct EditorLayout {
    graph: Rect,
    sources: Rect,
    plot: Rect,
}

/// Construct the Toybox Radiant host facade used by the macOS VST3 controller.
pub(crate) fn new_gui(shared: Arc<ChromascopeShared>) -> toybox::radiant_gui::RadiantHostedGui {
    toybox::radiant_gui::RadiantHostedGui::new(
        "ChromascopeRadiantVst3Editor",
        ChromascopeRadiantEditor::new(shared),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    )
    .with_size_contract(
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (WINDOW_WIDTH * 2, WINDOW_HEIGHT * 2),
    )
}

/// Retained editor state rendered by Toybox's macOS AppKit bridge.
struct ChromascopeRadiantEditor {
    shared: Arc<ChromascopeShared>,
    size: Vector2,
    selected_ids: Vec<u64>,
    highlighted: Vec<HighlightedSource>,
    highlight_limit_reached: bool,
    source_scroll_offset: f32,
    presentation: PresentationState,
    last_paint_at: Option<Instant>,
    activity_blink_phase_seconds: f32,
    paint_plan: SurfacePaintPlan,
}

impl ChromascopeRadiantEditor {
    fn new(shared: Arc<ChromascopeShared>) -> Self {
        Self {
            shared,
            size: Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32),
            selected_ids: Vec::new(),
            highlighted: Vec::new(),
            highlight_limit_reached: false,
            source_scroll_offset: 0.0,
            presentation: PresentationState::default(),
            last_paint_at: None,
            activity_blink_phase_seconds: 0.0,
            paint_plan: SurfacePaintPlan {
                clear_color: BACKGROUND,
                primitives: Vec::with_capacity(256),
            },
        }
    }

    fn toggle_companion(&mut self, id: u64, active: bool) {
        if !active {
            return;
        }
        if let Some(index) = self
            .selected_ids
            .iter()
            .position(|selected| *selected == id)
        {
            self.selected_ids.remove(index);
            crate::registry::set_companion_analysis_interest(id, false);
        } else if self.selected_ids.len() < MAX_COMPANIONS {
            crate::registry::set_companion_analysis_interest(id, true);
            self.selected_ids.push(id);
        }
    }

    fn activate_companion(&mut self, id: u64, active: bool) {
        if active && !self.selected_ids.contains(&id) && self.selected_ids.len() < MAX_COMPANIONS {
            crate::registry::set_companion_analysis_interest(id, true);
            self.selected_ids.push(id);
        }
    }

    fn toggle_highlight(&mut self, id: u64, active: bool) -> HighlightToggle {
        let outcome = toggle_highlight(&mut self.highlighted, id, active);
        match outcome {
            HighlightToggle::AtCapacity => self.highlight_limit_reached = true,
            HighlightToggle::Added | HighlightToggle::Removed => {
                self.highlight_limit_reached = false;
            }
            HighlightToggle::Ignored => {}
        }
        outcome
    }

    fn companion_at(
        &self,
        position: Point,
        companions: &[CompanionSourceSnapshot],
    ) -> Option<(u64, bool)> {
        let layout = editor_layout(self.size);
        let list = source_list_rect(layout.sources);
        if !list.contains(position) {
            return None;
        }
        visible_source_range(companions.len(), list.height(), self.source_scroll_offset)
            .filter_map(|index| companions.get(index).map(|source| (index, source)))
            .find_map(|(index, source)| {
                source_row_rect(layout.sources, index, self.source_scroll_offset)
                    .contains(position)
                    .then_some((source.id, source.active))
            })
    }

    fn build_paint_plan(&mut self) {
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
        let layout = editor_layout(self.size);
        self.prune_stale_selections(&companions);
        self.source_scroll_offset =
            clamp_source_scroll(layout.sources, companions.len(), self.source_scroll_offset);
        let selected_ids = self
            .selected_ids
            .iter()
            .copied()
            .filter(|id| companions.iter().any(|source| source.id == *id))
            .collect::<Vec<_>>();
        self.prune_stale_highlights(&companions);
        self.presentation.retain_selected(&selected_ids);
        let now = Instant::now();
        let elapsed_seconds = self
            .last_paint_at
            .replace(now)
            .map(|previous| now.duration_since(previous).as_secs_f32())
            .unwrap_or(PRESENTATION_FRAME_SECONDS);
        self.activity_blink_phase_seconds = (self.activity_blink_phase_seconds + elapsed_seconds)
            .rem_euclid(COMPANION_ACTIVITY_BLINK_PERIOD_SECONDS);
        let activities = snapshot_companion_activity();

        self.paint_plan.clear_color = BACKGROUND;
        self.paint_plan.primitives.clear();
        paint_editor_with_presentation(
            &mut self.paint_plan,
            layout,
            main_frame,
            &companions,
            &selected_ids,
            &self.highlighted,
            &activities,
            self.source_scroll_offset,
            &mut self.presentation,
            elapsed_seconds,
            self.activity_blink_phase_seconds,
            self.highlight_limit_reached,
        );
    }

    fn prune_stale_selections(&mut self, companions: &[CompanionSourceSnapshot]) {
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

    fn prune_stale_highlights(&mut self, companions: &[CompanionSourceSnapshot]) {
        let before = self.highlighted.len();
        self.highlighted.retain(|highlight| {
            companions
                .iter()
                .any(|source| source.id == highlight.id && source.active)
        });
        if self.highlighted.len() != before {
            self.highlight_limit_reached = false;
        }
    }
}

impl Drop for ChromascopeRadiantEditor {
    fn drop(&mut self) {
        for id in self.selected_ids.drain(..) {
            crate::registry::set_companion_analysis_interest(id, false);
        }
    }
}

impl toybox::radiant_gui::RadiantEditor for ChromascopeRadiantEditor {
    fn resize(&mut self, width: u32, height: u32) {
        self.size = Vector2::new(width.max(1) as f32, height.max(1) as f32);
    }

    fn dispatch_event(&mut self, event: Event) {
        match event {
            Event::Resize { viewport } => {
                self.size = Vector2::new(viewport.x.max(1.0), viewport.y.max(1.0));
                self.source_scroll_offset = clamp_source_scroll(
                    editor_layout(self.size).sources,
                    crate::registry::snapshot_companions().len(),
                    self.source_scroll_offset,
                );
            }
            Event::Scroll {
                position, delta, ..
            } => {
                let layout = editor_layout(self.size);
                let list = source_list_rect(layout.sources);
                if list.contains(position) {
                    let count = crate::registry::snapshot_companions().len();
                    self.source_scroll_offset = clamp_source_scroll(
                        layout.sources,
                        count,
                        self.source_scroll_offset + delta.y,
                    );
                }
            }
            Event::PointerPress {
                position,
                button: PointerButton::Primary,
                modifiers,
            } => {
                let companions = crate::registry::snapshot_companions();
                if let Some((id, active)) = self.companion_at(position, &companions) {
                    if modifiers.command {
                        if matches!(self.toggle_highlight(id, active), HighlightToggle::Added) {
                            self.activate_companion(id, active);
                        }
                    } else {
                        self.toggle_companion(id, active);
                    }
                }
            }
            _ => {}
        }
    }

    fn paint_plan(&mut self) -> &SurfacePaintPlan {
        self.build_paint_plan();
        &self.paint_plan
    }

    fn needs_realtime_redraw(&self) -> bool {
        // The viewer must reflect new FFT frames and companion lifecycle changes
        // without asking the DAW for track enumeration or selected-track state.
        true
    }

    fn dispatch_key_press(&mut self, _key: WidgetKey) -> bool {
        false
    }

    fn dispatch_character(&mut self, _character: char) -> bool {
        false
    }

    fn cancel_text_entry(&mut self) -> bool {
        false
    }
}

fn editor_layout(size: Vector2) -> EditorLayout {
    let width = size.x.max(1.0);
    let height = size.y.max(1.0);
    let content = Rect::from_xy_size(
        OUTER_MARGIN,
        OUTER_MARGIN,
        (width - OUTER_MARGIN * 2.0).max(1.0),
        (height - OUTER_MARGIN * 2.0).max(1.0),
    );
    let source_width = (content.width() * SOURCE_PANEL_RATIO)
        .max(SOURCE_PANEL_MIN_WIDTH)
        .min(content.width() * SOURCE_PANEL_MAX_RATIO);
    let graph_width = (content.width() - PANEL_GAP - source_width).max(1.0);
    let graph = Rect::from_xy_size(content.min.x, content.min.y, graph_width, content.height());
    let sources = Rect::from_xy_size(
        graph.max.x + PANEL_GAP,
        content.min.y,
        source_width,
        content.height(),
    );
    let plot = Rect::from_xy_size(
        graph.min.x + PLOT_PADDING_LEFT,
        graph.min.y + GRAPH_HEADER_HEIGHT + PLOT_PADDING_TOP,
        (graph.width() - PLOT_PADDING_LEFT - PLOT_PADDING_RIGHT).max(1.0),
        (graph.height() - GRAPH_HEADER_HEIGHT - PLOT_PADDING_TOP - PLOT_PADDING_BOTTOM).max(1.0),
    );
    EditorLayout {
        graph,
        sources,
        plot,
    }
}

#[cfg(test)]
fn paint_editor(
    plan: &mut SurfacePaintPlan,
    layout: EditorLayout,
    main: Option<SpectrumFrame>,
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
    source_scroll_offset: f32,
) {
    let mut presentation = PresentationState::default();
    paint_editor_with_presentation(
        plan,
        layout,
        main,
        companions,
        selected_ids,
        &[],
        &[],
        source_scroll_offset,
        &mut presentation,
        PRESENTATION_FRAME_SECONDS,
        0.0,
        false,
    );
}

#[allow(clippy::too_many_arguments)]
fn paint_editor_with_presentation(
    plan: &mut SurfacePaintPlan,
    layout: EditorLayout,
    main: Option<SpectrumFrame>,
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
    highlighted: &[HighlightedSource],
    activities: &[CompanionActivitySnapshot],
    source_scroll_offset: f32,
    presentation: &mut PresentationState,
    elapsed_seconds: f32,
    activity_blink_phase_seconds: f32,
    highlight_limit_reached: bool,
) {
    push_fill(
        plan,
        ROOT_WIDGET_ID,
        Rect::from_xy_size(0.0, 0.0, layout_size(layout).x, layout_size(layout).y),
        BACKGROUND,
    );
    push_rounded_fill(
        plan,
        GRAPH_WIDGET_ID,
        layout.graph,
        PANEL_BACKGROUND,
        PUMP_ALIGNED_METRICS.radius,
    );
    push_rounded_outline(
        plan,
        GRAPH_WIDGET_ID,
        layout.graph,
        BORDER,
        PUMP_ALIGNED_METRICS.radius,
        PUMP_ALIGNED_METRICS.border,
    );
    push_rounded_fill(
        plan,
        SOURCE_WIDGET_ID,
        layout.sources,
        PANEL_BACKGROUND,
        PUMP_ALIGNED_METRICS.radius,
    );
    push_rounded_outline(
        plan,
        SOURCE_WIDGET_ID,
        layout.sources,
        BORDER,
        PUMP_ALIGNED_METRICS.radius,
        PUMP_ALIGNED_METRICS.border,
    );
    push_fill(
        plan,
        GRAPH_WIDGET_ID,
        Rect::from_xy_size(
            layout.graph.min.x,
            layout.graph.min.y + GRAPH_HEADER_HEIGHT - PUMP_ALIGNED_METRICS.border,
            layout.graph.width(),
            PUMP_ALIGNED_METRICS.border,
        ),
        BORDER,
    );
    push_fill(
        plan,
        SOURCE_WIDGET_ID,
        Rect::from_xy_size(
            layout.sources.min.x,
            layout.sources.min.y + SOURCE_HEADER_HEIGHT - PUMP_ALIGNED_METRICS.border,
            layout.sources.width(),
            PUMP_ALIGNED_METRICS.border,
        ),
        BORDER,
    );

    push_text(
        plan,
        ROOT_WIDGET_ID,
        PLUGIN_NAME,
        Rect::from_xy_size(
            layout.graph.min.x + PUMP_ALIGNED_METRICS.space_16,
            layout.graph.min.y + 7.0,
            layout.graph.width() - 28.0,
            24.0,
        ),
        BRAND_FONT_SIZE,
        SOFT_LABEL,
        PaintTextAlign::Left,
    );
    push_text(
        plan,
        GRAPH_WIDGET_ID,
        SPECTRUM_HEADER,
        Rect::from_xy_size(
            layout.graph.min.x + PUMP_ALIGNED_METRICS.space_16,
            layout.graph.min.y + 28.0,
            layout.graph.width() - 28.0,
            PUMP_ALIGNED_TYPOGRAPHY.body.1,
        ),
        BODY_FONT_SIZE,
        LABEL,
        PaintTextAlign::Left,
    );
    push_text(
        plan,
        SOURCE_WIDGET_ID,
        format!(
            "SOURCES  {}/{}  •  HIGHLIGHTS  {}/{}",
            selected_ids.len(),
            MAX_COMPANIONS,
            highlighted.len(),
            MAX_HIGHLIGHTS,
        ),
        Rect::from_xy_size(
            layout.sources.min.x + PUMP_ALIGNED_METRICS.space_16,
            layout.sources.min.y + 7.0,
            layout.sources.width() - 28.0,
            20.0,
        ),
        BODY_FONT_SIZE,
        SOFT_LABEL,
        PaintTextAlign::Left,
    );

    paint_graph(
        plan,
        layout.plot,
        main,
        companions,
        selected_ids,
        highlighted,
        presentation,
        elapsed_seconds,
    );
    paint_sources(
        plan,
        layout.sources,
        companions,
        selected_ids,
        highlighted,
        activities,
        source_scroll_offset,
        activity_blink_phase_seconds,
        highlight_limit_reached,
    );
    push_rounded_outline(
        plan,
        ROOT_WIDGET_ID,
        Rect::from_xy_size(
            0.5,
            0.5,
            layout_size(layout).x - 1.0,
            layout_size(layout).y - 1.0,
        ),
        BORDER_EMPHASIS,
        PUMP_ALIGNED_METRICS.radius,
        PUMP_ALIGNED_METRICS.border,
    );
}

fn layout_size(layout: EditorLayout) -> Vector2 {
    Vector2::new(
        (layout.sources.max.x + OUTER_MARGIN).max(layout.graph.max.x + OUTER_MARGIN),
        (layout.sources.max.y + OUTER_MARGIN).max(layout.graph.max.y + OUTER_MARGIN),
    )
}

#[allow(clippy::too_many_arguments)]
fn paint_graph(
    plan: &mut SurfacePaintPlan,
    plot: Rect,
    main: Option<SpectrumFrame>,
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
    highlighted: &[HighlightedSource],
    presentation: &mut PresentationState,
    elapsed_seconds: f32,
) {
    push_fill(plan, GRAPH_WIDGET_ID, plot, BACKGROUND);
    push_stroke_rect(plan, GRAPH_WIDGET_ID, plot, BORDER, 1.0);
    draw_grid(plan, plot);

    for companion in companions {
        if companion.active
            && selected_ids.contains(&companion.id)
            && let Some(frame) = companion.frame
            && let Some(frame) =
                presentation.advance_companion(companion.id, Some(frame), elapsed_seconds)
        {
            push_polyline(
                plan,
                GRAPH_WIDGET_ID,
                trace_points(&frame, plot),
                to_color(color_for(highlighted, companion.id).unwrap_or(companion.color)),
                COMPANION_TRACE_THICKNESS,
            );
        }
    }

    if let Some(frame) = presentation.advance_main(main, elapsed_seconds) {
        push_polyline(
            plan,
            GRAPH_WIDGET_ID,
            trace_points(&frame, plot),
            to_color(MAIN_SPECTRUM_COLOR),
            MAIN_TRACE_THICKNESS,
        );
    } else {
        push_text(
            plan,
            GRAPH_WIDGET_ID,
            "WAITING FOR MAIN INPUT",
            Rect::from_xy_size(
                plot.min.x + 16.0,
                plot.center().y - 8.0,
                plot.width() - 32.0,
                18.0,
            ),
            BODY_FONT_SIZE,
            LABEL,
            PaintTextAlign::Center,
        );
    }
}

fn draw_grid(plan: &mut SurfacePaintPlan, plot: Rect) {
    for frequency in [20.0, 100.0, 1_000.0, 10_000.0] {
        for multiplier in 2..10 {
            let minor_frequency = frequency * multiplier as f32;
            if minor_frequency < MAX_FREQUENCY_HZ {
                let x = frequency_to_x(minor_frequency, plot);
                push_fill(
                    plan,
                    GRAPH_WIDGET_ID,
                    Rect::from_xy_size(x, plot.min.y, 1.0, plot.height()),
                    MINOR_GRID,
                );
            }
        }
    }

    for frequency in [20.0, 100.0, 1_000.0, 10_000.0, 20_000.0] {
        let x = frequency_to_x(frequency, plot);
        push_fill(
            plan,
            GRAPH_WIDGET_ID,
            Rect::from_xy_size(x, plot.min.y, 1.0, plot.height()),
            MAJOR_GRID,
        );
        push_text(
            plan,
            GRAPH_WIDGET_ID,
            frequency_label(frequency),
            Rect::from_xy_size(x - 18.0, plot.max.y + 7.0, 36.0, 16.0),
            META_FONT_SIZE,
            LABEL,
            PaintTextAlign::Center,
        );
    }

    for level in (-100..=0).step_by(20) {
        let y = level_to_y(level as f32, plot);
        push_fill(
            plan,
            GRAPH_WIDGET_ID,
            Rect::from_xy_size(plot.min.x, y, plot.width(), 1.0),
            MAJOR_GRID,
        );
        push_text(
            plan,
            GRAPH_WIDGET_ID,
            format!("{level}"),
            Rect::from_xy_size(plot.min.x - 38.0, y - 7.0, 32.0, 16.0),
            META_FONT_SIZE,
            LABEL,
            PaintTextAlign::Right,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_sources(
    plan: &mut SurfacePaintPlan,
    panel: Rect,
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
    highlighted: &[HighlightedSource],
    activities: &[CompanionActivitySnapshot],
    source_scroll_offset: f32,
    activity_blink_phase_seconds: f32,
    highlight_limit_reached: bool,
) {
    let legend = Rect::from_xy_size(
        panel.min.x + 10.0,
        panel.min.y + SOURCE_HEADER_HEIGHT + 8.0,
        panel.width() - 20.0,
        SOURCE_LEGEND_HEIGHT - 8.0,
    );
    push_fill(plan, SOURCE_WIDGET_ID, legend, BACKGROUND);
    push_fill(
        plan,
        SOURCE_WIDGET_ID,
        Rect::from_xy_size(legend.min.x + 10.0, legend.center().y - 1.0, 28.0, 2.0),
        to_color(MAIN_SPECTRUM_COLOR),
    );
    push_text(
        plan,
        SOURCE_WIDGET_ID,
        "MAIN INPUT",
        Rect::from_xy_size(
            legend.min.x + 48.0,
            legend.min.y + 8.0,
            legend.width() - 58.0,
            16.0,
        ),
        VALUE_FONT_SIZE,
        to_color(MAIN_SPECTRUM_COLOR),
        PaintTextAlign::Left,
    );
    push_text(
        plan,
        SOURCE_WIDGET_ID,
        if highlight_limit_reached {
            "HIGHLIGHTS FULL  ·  REMOVE ONE TO ADD"
        } else {
            "COMPANIONS  ·  SCROLL  ·  ⌘ CLICK HIGHLIGHT"
        },
        Rect::from_xy_size(
            panel.min.x + PUMP_ALIGNED_METRICS.space_16,
            panel.min.y + SOURCE_HEADER_HEIGHT + SOURCE_LEGEND_HEIGHT,
            panel.width() - 28.0,
            18.0,
        ),
        CONTROL_LABEL_FONT_SIZE,
        LABEL,
        PaintTextAlign::Left,
    );

    let list = source_list_rect(panel);
    if companions.is_empty() {
        push_text(
            plan,
            SOURCE_WIDGET_ID,
            "NO COMPANIONS REGISTERED",
            Rect::from_xy_size(list.min.x, list.min.y + 6.0, list.width() - 16.0, 18.0),
            CONTROL_LABEL_FONT_SIZE,
            MUTED,
            PaintTextAlign::Left,
        );
        return;
    }

    let source_scroll_offset = clamp_source_scroll(panel, companions.len(), source_scroll_offset);
    plan.primitives
        .push(PaintPrimitive::ClipStart(PaintClipStart {
            node_id: SOURCE_WIDGET_ID,
            rect: list,
        }));
    for index in visible_source_range(companions.len(), list.height(), source_scroll_offset) {
        let Some(source) = companions.get(index) else {
            continue;
        };
        let row = source_row_rect(panel, index, source_scroll_offset);
        let selected = selected_ids.contains(&source.id);
        let highlight_color = color_for(highlighted, source.id).map(to_color);
        if selected {
            let selected_row = row.inset(0.5, 0.5, 0.5, 0.5);
            push_rounded_fill(
                plan,
                SOURCE_WIDGET_ID,
                selected_row,
                SELECTED_ROW,
                PUMP_ALIGNED_METRICS.base,
            );
            push_rounded_outline(
                plan,
                SOURCE_WIDGET_ID,
                selected_row,
                to_color(PUMP_PALETTE.accent_secondary).with_alpha(180),
                PUMP_ALIGNED_METRICS.base,
                PUMP_ALIGNED_METRICS.border,
            );
        }
        if let Some(highlight_color) = highlight_color {
            let highlighted_row = row.inset(0.5, 0.5, 0.5, 0.5);
            push_rounded_fill(
                plan,
                SOURCE_WIDGET_ID,
                highlighted_row,
                highlight_color.with_alpha(32),
                PUMP_ALIGNED_METRICS.base,
            );
            push_rounded_outline(
                plan,
                SOURCE_WIDGET_ID,
                highlighted_row,
                highlight_color.with_alpha(220),
                PUMP_ALIGNED_METRICS.base,
                PUMP_ALIGNED_METRICS.border,
            );
        }
        let marker_color = highlight_color
            .unwrap_or_else(|| to_color(source.color))
            .with_alpha_if(source.active, 255, 96);
        push_fill(
            plan,
            SOURCE_WIDGET_ID,
            Rect::from_xy_size(row.min.x + 10.0, row.center().y - 4.0, 8.0, 8.0),
            marker_color,
        );
        if source.active
            && activity_blink_on(activity_blink_phase_seconds)
            && source_input_active(source.id, activities)
        {
            push_fill(
                plan,
                SOURCE_WIDGET_ID,
                Rect::from_xy_size(row.min.x + 21.0, row.center().y - 3.0, 6.0, 6.0),
                ACTIVITY_DOT,
            );
        }
        push_text(
            plan,
            SOURCE_WIDGET_ID,
            source.name.clone(),
            Rect::from_xy_size(row.min.x + 32.0, row.min.y + 6.0, row.width() * 0.45, 16.0),
            VALUE_FONT_SIZE,
            highlight_color.unwrap_or(if source.active { SOFT_LABEL } else { INACTIVE }),
            PaintTextAlign::Left,
        );
        let (status, status_color) = if !source.active {
            ("INACTIVE", INACTIVE)
        } else if !source.analysis_requested {
            ("IDLE", MUTED)
        } else if source.frame.is_some() {
            ("LIVE", LIVE)
        } else {
            ("WARMING", WARMING)
        };
        push_text(
            plan,
            SOURCE_WIDGET_ID,
            status,
            Rect::from_xy_size(
                row.min.x + row.width() * 0.56,
                row.min.y + 6.0,
                row.width() * 0.25,
                16.0,
            ),
            META_FONT_SIZE,
            status_color,
            PaintTextAlign::Left,
        );
        let checkbox = Rect::from_xy_size(row.max.x - 28.0, row.center().y - 8.0, 18.0, 18.0);
        push_stroke_rect(
            plan,
            SOURCE_WIDGET_ID,
            checkbox,
            if source.active {
                to_color(source.color)
            } else {
                INACTIVE
            },
            1.0,
        );
        if selected {
            push_fill(
                plan,
                SOURCE_WIDGET_ID,
                Rect::from_xy_size(checkbox.min.x + 3.0, checkbox.min.y + 3.0, 12.0, 12.0),
                if source.active {
                    to_color(source.color)
                } else {
                    INACTIVE
                },
            );
            push_text(
                plan,
                SOURCE_WIDGET_ID,
                "✓",
                Rect::from_xy_size(checkbox.min.x, checkbox.min.y + 1.0, checkbox.width(), 16.0),
                VALUE_FONT_SIZE,
                BACKGROUND,
                PaintTextAlign::Center,
            );
        }
    }
    plan.primitives.push(PaintPrimitive::ClipEnd(PaintClipEnd {
        node_id: SOURCE_WIDGET_ID,
    }));
    paint_source_scrollbar(plan, panel, companions.len(), source_scroll_offset);
}

fn source_list_rect(panel: Rect) -> Rect {
    let origin_y =
        panel.min.y + SOURCE_HEADER_HEIGHT + SOURCE_LEGEND_HEIGHT + SOURCE_LIST_HEADER_HEIGHT;
    Rect::from_xy_size(
        panel.min.x + 8.0,
        origin_y,
        (panel.width() - 16.0).max(1.0),
        (panel.max.y - origin_y - 8.0).max(1.0),
    )
}

fn source_content_height(companion_count: usize) -> f32 {
    if companion_count == 0 {
        0.0
    } else {
        companion_count as f32 * (SOURCE_ROW_HEIGHT + SOURCE_ROW_GAP) - SOURCE_ROW_GAP
    }
}

fn source_input_active(id: u64, activities: &[CompanionActivitySnapshot]) -> bool {
    activities
        .iter()
        .any(|activity| activity.id == id && activity.input_active)
}

fn activity_blink_on(phase_seconds: f32) -> bool {
    phase_seconds.rem_euclid(COMPANION_ACTIVITY_BLINK_PERIOD_SECONDS)
        < COMPANION_ACTIVITY_BLINK_PERIOD_SECONDS * 0.5
}

fn max_source_scroll(panel: Rect, companion_count: usize) -> f32 {
    // Keep the last fractional-height row just inside the clip rect. The
    // explicit positive epsilon absorbs f32 rounding from the Pump-derived
    // 27.2/1.7 geometry without creating a visible gap at the end of the list.
    (source_content_height(companion_count) - source_list_rect(panel).height()
        + SOURCE_SCROLL_EPSILON)
        .max(0.0)
}

fn clamp_source_scroll(panel: Rect, companion_count: usize, offset: f32) -> f32 {
    offset
        .max(0.0)
        .min(max_source_scroll(panel, companion_count))
}

fn visible_source_range(
    companion_count: usize,
    viewport_height: f32,
    scroll_offset: f32,
) -> std::ops::Range<usize> {
    if companion_count == 0 || viewport_height <= 0.0 {
        return 0..0;
    }
    let row_extent = SOURCE_ROW_HEIGHT + SOURCE_ROW_GAP;
    let first = (scroll_offset / row_extent).floor() as usize;
    let last = ((scroll_offset + viewport_height) / row_extent).ceil() as usize + 1;
    first.min(companion_count)..last.min(companion_count)
}

fn source_row_rect(panel: Rect, index: usize, scroll_offset: f32) -> Rect {
    let y = panel.min.y
        + SOURCE_HEADER_HEIGHT
        + SOURCE_LEGEND_HEIGHT
        + SOURCE_LIST_HEADER_HEIGHT
        + index as f32 * (SOURCE_ROW_HEIGHT + SOURCE_ROW_GAP)
        - scroll_offset;
    Rect::from_xy_size(
        panel.min.x + 8.0,
        y,
        panel.width() - 16.0,
        SOURCE_ROW_HEIGHT,
    )
}

fn paint_source_scrollbar(
    plan: &mut SurfacePaintPlan,
    panel: Rect,
    companion_count: usize,
    scroll_offset: f32,
) {
    let list = source_list_rect(panel);
    let content_height = source_content_height(companion_count);
    if content_height <= list.height() {
        return;
    }
    let track = Rect::from_xy_size(list.max.x - 5.0, list.min.y, 3.0, list.height());
    let thumb_height = (list.height() * list.height() / content_height)
        .max(18.0)
        .min(list.height());
    let scroll_span = (list.height() - thumb_height).max(0.0);
    let thumb_y = list.min.y
        + scroll_span * (scroll_offset / max_source_scroll(panel, companion_count).max(1.0));
    push_fill(plan, SOURCE_WIDGET_ID, track, SCROLL_TRACK);
    push_fill(
        plan,
        SOURCE_WIDGET_ID,
        Rect::from_xy_size(track.min.x, thumb_y, track.width(), thumb_height),
        SCROLL_THUMB,
    );
}

fn trace_points(frame: &SpectrumFrame, plot: Rect) -> Vec<Point> {
    let low_frequency = crate::analysis::band_center_frequency_hz(0, 48_000.0);
    let high_frequency = crate::analysis::band_center_frequency_hz(MAX_BANDS - 1, 48_000.0);
    (0..DISPLAY_TRACE_SAMPLES)
        .map(|sample| {
            let position = display_trace_band_position(sample);
            let frequency = low_frequency
                * (high_frequency / low_frequency)
                    .powf(position / MAX_BANDS.saturating_sub(1).max(1) as f32);
            Point::new(
                frequency_to_x(frequency, plot),
                level_to_y(interpolated_band_value(&frame.values, position), plot),
            )
        })
        .collect()
}

fn frequency_to_x(frequency: f32, plot: Rect) -> f32 {
    let minimum = MIN_FREQUENCY_HZ.log10();
    let maximum = MAX_FREQUENCY_HZ.log10();
    let ratio = ((frequency.clamp(MIN_FREQUENCY_HZ, MAX_FREQUENCY_HZ).log10() - minimum)
        / (maximum - minimum))
        .clamp(0.0, 1.0);
    plot.min.x + ratio * plot.width().max(1.0)
}

fn level_to_y(level: f32, plot: Rect) -> f32 {
    let ratio = ((MAX_LEVEL_DB - level.clamp(MIN_LEVEL_DB, MAX_LEVEL_DB))
        / (MAX_LEVEL_DB - MIN_LEVEL_DB))
        .clamp(0.0, 1.0);
    plot.min.y + ratio * plot.height().max(1.0)
}

fn frequency_label(frequency: f32) -> &'static str {
    match frequency as u32 {
        20 => "20",
        100 => "100",
        1_000 => "1k",
        10_000 => "10k",
        _ => "20k",
    }
}

fn push_fill(plan: &mut SurfacePaintPlan, widget_id: u64, rect: Rect, color: Rgba8) {
    plan.primitives
        .push(PaintPrimitive::FillRect(PaintFillRect {
            widget_id,
            rect,
            color,
        }));
}

fn push_rounded_fill(
    plan: &mut SurfacePaintPlan,
    widget_id: u64,
    rect: Rect,
    color: Rgba8,
    radius: f32,
) {
    plan.primitives
        .push(PaintPrimitive::FillPath(PaintFillPath::new(
            widget_id,
            PaintPath::from(rounded_rect_commands(rect, radius)),
            PaintBrush::solid(color),
        )));
}

fn push_rounded_outline(
    plan: &mut SurfacePaintPlan,
    widget_id: u64,
    rect: Rect,
    color: Rgba8,
    radius: f32,
    width: f32,
) {
    plan.primitives.push(PaintPrimitive::FillPath(
        PaintFillPath::new(
            widget_id,
            rounded_ring_path(rect, radius, width),
            PaintBrush::solid(color),
        )
        .fill_rule(PaintFillRule::EvenOdd),
    ));
}

fn rounded_rect_commands(rect: Rect, radius: f32) -> Vec<PaintPathCommand> {
    let left = rect.min.x;
    let right = rect.max.x;
    let top = rect.min.y;
    let bottom = rect.max.y;
    let radius = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let control = 0.552_284_8 * radius;
    vec![
        PaintPathCommand::MoveTo(Point::new(left + radius, top)),
        PaintPathCommand::LineTo(Point::new(right - radius, top)),
        PaintPathCommand::CurveTo {
            control1: Point::new(right - radius + control, top),
            control2: Point::new(right, top + radius - control),
            to: Point::new(right, top + radius),
        },
        PaintPathCommand::LineTo(Point::new(right, bottom - radius)),
        PaintPathCommand::CurveTo {
            control1: Point::new(right, bottom - radius + control),
            control2: Point::new(right - radius + control, bottom),
            to: Point::new(right - radius, bottom),
        },
        PaintPathCommand::LineTo(Point::new(left + radius, bottom)),
        PaintPathCommand::CurveTo {
            control1: Point::new(left + radius - control, bottom),
            control2: Point::new(left, bottom - radius + control),
            to: Point::new(left, bottom - radius),
        },
        PaintPathCommand::LineTo(Point::new(left, top + radius)),
        PaintPathCommand::CurveTo {
            control1: Point::new(left, top + radius - control),
            control2: Point::new(left + radius - control, top),
            to: Point::new(left + radius, top),
        },
        PaintPathCommand::Close,
    ]
}

fn rounded_ring_path(rect: Rect, radius: f32, width: f32) -> PaintPath {
    let width = width.max(0.0);
    let inner = rect.inset(width, width, width, width);
    let outer_radius = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let inner_radius = (outer_radius - width)
        .min(inner.width() * 0.5)
        .min(inner.height() * 0.5)
        .max(0.0);
    let mut commands = rounded_rect_commands(rect, outer_radius);
    commands.extend(rounded_rect_commands(inner, inner_radius));
    PaintPath::from(commands)
}

fn push_stroke_rect(
    plan: &mut SurfacePaintPlan,
    widget_id: u64,
    rect: Rect,
    color: Rgba8,
    width: f32,
) {
    plan.primitives
        .push(PaintPrimitive::StrokeRect(PaintStrokeRect {
            widget_id,
            rect,
            color,
            width,
        }));
}

fn push_polyline(
    plan: &mut SurfacePaintPlan,
    widget_id: u64,
    points: Vec<Point>,
    color: Rgba8,
    width: f32,
) {
    plan.primitives
        .push(PaintPrimitive::StrokePolyline(PaintStrokePolyline {
            widget_id,
            points: Arc::from(points),
            color,
            width,
        }));
}

fn push_text(
    plan: &mut SurfacePaintPlan,
    widget_id: u64,
    text: impl Into<PaintText>,
    rect: Rect,
    font_size: f32,
    color: Rgba8,
    align: PaintTextAlign,
) {
    plan.primitives.push(PaintPrimitive::Text(PaintTextRun {
        widget_id,
        text: text.into(),
        rect,
        font_size,
        baseline: Some(font_size),
        color,
        align,
        wrap: TextWrap::None,
    }));
}

fn to_color(rgb: Rgb) -> Rgba8 {
    Rgba8::new(rgb.red, rgb.green, rgb.blue, 255)
}

#[cfg(test)]
mod tests {
    use super::*;
    use radiant::widgets::PointerModifiers;
    use toybox::radiant_gui::RadiantEditor;

    fn frame(value: f32) -> SpectrumFrame {
        SpectrumFrame {
            sequence: 1,
            sample_position: None,
            values: [value; MAX_BANDS],
        }
    }

    fn source(id: u64, active: bool, frame: Option<SpectrumFrame>) -> CompanionSourceSnapshot {
        CompanionSourceSnapshot {
            id,
            name: format!("COMPANION {id}"),
            color: crate::registry::color_from_seed(id),
            active,
            analysis_requested: active,
            frame,
        }
    }

    #[test]
    fn radiant_plan_contains_visible_editor_and_source_controls() {
        let size = Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32);
        let layout = editor_layout(size);
        let mut plan = SurfacePaintPlan {
            clear_color: BACKGROUND,
            primitives: Vec::new(),
        };
        paint_editor(
            &mut plan,
            layout,
            Some(frame(-12.0)),
            &[source(7, true, Some(frame(-24.0))), source(8, false, None)],
            &[7],
            0.0,
        );

        let stats = plan.stats();
        assert!(stats.total > 0);
        assert!(stats.text >= 8);
        assert!(plan.primitives.iter().any(|primitive| {
            matches!(
                primitive,
                    PaintPrimitive::StrokePolyline(polyline)
                    if polyline.color == to_color(MAIN_SPECTRUM_COLOR)
                        && polyline.width == MAIN_TRACE_THICKNESS
            )
        }));
        assert!(plan.primitives.iter().any(|primitive| {
            matches!(primitive, PaintPrimitive::Text(text) if text.text.as_str() == "COMPANION 7")
        }));
    }

    #[test]
    fn native_editor_requests_periodic_repaint_for_presentation() {
        let editor = ChromascopeRadiantEditor::new(Arc::new(ChromascopeShared::new(
            crate::shared::DeviceKind::Viewer,
        )));
        assert!(editor.needs_realtime_redraw());
    }

    #[test]
    fn native_source_paint_materializes_only_visible_rows_and_no_unselected_traces() {
        let size = Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32);
        let layout = editor_layout(size);
        let list = source_list_rect(layout.sources);
        let companions: Vec<_> = (0..MAX_COMPANIONS)
            .map(|index| source(index as u64 + 10_000, true, Some(frame(-20.0))))
            .collect();
        let expected_rows = visible_source_range(MAX_COMPANIONS, list.height(), 0.0).len();
        let mut plan = SurfacePaintPlan {
            clear_color: BACKGROUND,
            primitives: Vec::new(),
        };

        paint_editor(&mut plan, layout, Some(frame(-12.0)), &companions, &[], 0.0);

        let row_labels = plan
            .primitives
            .iter()
            .filter(|primitive| {
                matches!(
                    primitive,
                    PaintPrimitive::Text(text) if text.text.as_str().starts_with("COMPANION ")
                )
            })
            .count();
        let companion_traces = plan
            .primitives
            .iter()
            .filter(|primitive| {
                matches!(
                    primitive,
                    PaintPrimitive::StrokePolyline(polyline)
                        if polyline.width == COMPANION_TRACE_THICKNESS
                )
            })
            .count();

        assert_eq!(row_labels, expected_rows);
        assert_eq!(companion_traces, 0);
        assert!(
            plan.primitives
                .iter()
                .any(|primitive| matches!(primitive, PaintPrimitive::ClipStart(_)))
        );
    }

    #[test]
    fn native_source_rows_blink_only_the_atomic_pre_fader_activity_marker() {
        let size = Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32);
        let layout = editor_layout(size);
        let companions = [source(7, true, None)];
        let activities = [CompanionActivitySnapshot {
            id: 7,
            input_active: true,
        }];
        let mut presentation = PresentationState::default();
        let mut lit_plan = SurfacePaintPlan {
            clear_color: BACKGROUND,
            primitives: Vec::new(),
        };
        paint_editor_with_presentation(
            &mut lit_plan,
            layout,
            Some(frame(-12.0)),
            &companions,
            &[],
            &[],
            &activities,
            0.0,
            &mut presentation,
            PRESENTATION_FRAME_SECONDS,
            0.0,
            false,
        );

        let lit_activity_dots = lit_plan
            .primitives
            .iter()
            .filter(|primitive| {
                matches!(
                    primitive,
                    PaintPrimitive::FillRect(fill)
                        if fill.color == ACTIVITY_DOT
                            && fill.rect.width() == 6.0
                            && fill.rect.height() == 6.0
                )
            })
            .count();
        assert_eq!(lit_activity_dots, 1);
        assert!(lit_plan.primitives.iter().any(|primitive| {
            matches!(
                primitive,
                PaintPrimitive::FillRect(fill)
                    if fill.color == to_color(companions[0].color)
                        && fill.rect.width() == 8.0
                        && fill.rect.height() == 8.0
            )
        }));

        let mut dark_plan = SurfacePaintPlan {
            clear_color: BACKGROUND,
            primitives: Vec::new(),
        };
        paint_editor_with_presentation(
            &mut dark_plan,
            layout,
            Some(frame(-12.0)),
            &companions,
            &[],
            &[],
            &activities,
            0.0,
            &mut presentation,
            PRESENTATION_FRAME_SECONDS,
            COMPANION_ACTIVITY_BLINK_PERIOD_SECONDS * 0.75,
            false,
        );
        assert!(!dark_plan.primitives.iter().any(|primitive| {
            matches!(
                primitive,
                PaintPrimitive::FillRect(fill) if fill.color == ACTIVITY_DOT
            )
        }));
    }

    #[test]
    fn source_rows_are_stable_and_clickable_for_multiple_selection() {
        let layout = editor_layout(Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32));
        let companions = [source(11, true, None), source(12, true, None)];
        let first = source_row_rect(layout.sources, 0, 0.0).center();
        let second = source_row_rect(layout.sources, 1, 0.0).center();
        let mut editor = ChromascopeRadiantEditor::new(Arc::new(ChromascopeShared::new(
            crate::shared::DeviceKind::Viewer,
        )));

        for (position, source) in [(first, &companions[0]), (second, &companions[1])] {
            assert_eq!(
                editor.companion_at(position, &companions),
                Some((source.id, true))
            );
            editor.toggle_companion(source.id, source.active);
        }
        assert_eq!(editor.selected_ids, vec![11, 12]);
        editor.toggle_companion(11, true);
        assert_eq!(editor.selected_ids, vec![12]);
    }

    #[test]
    fn command_click_highlights_and_activates_source() {
        let handle = crate::registry::register_companion().expect("source");
        let id = handle.id();
        let mut editor = ChromascopeRadiantEditor::new(Arc::new(ChromascopeShared::new(
            crate::shared::DeviceKind::Viewer,
        )));
        let companions = crate::registry::snapshot_companions();
        let source_index = companions
            .iter()
            .position(|source| source.id == id)
            .expect("registered source should be snapshotted");
        let position = source_row_rect(
            editor_layout(Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32)).sources,
            source_index,
            0.0,
        )
        .center();

        editor.dispatch_event(Event::PointerPress {
            position,
            button: PointerButton::Primary,
            modifiers: PointerModifiers {
                command: true,
                ..PointerModifiers::default()
            },
        });
        assert_eq!(editor.selected_ids, vec![id]);
        assert_eq!(editor.highlighted.len(), 1);
        assert_eq!(editor.highlighted[0].id, id);

        editor.dispatch_event(Event::PointerPress {
            position,
            button: PointerButton::Primary,
            modifiers: PointerModifiers {
                command: true,
                ..PointerModifiers::default()
            },
        });
        assert_eq!(editor.selected_ids, vec![id]);
        assert!(editor.highlighted.is_empty());

        editor.dispatch_event(Event::PointerPress {
            position,
            button: PointerButton::Primary,
            modifiers: PointerModifiers::default(),
        });
        assert!(editor.highlighted.is_empty());
        assert!(editor.selected_ids.is_empty());
    }

    #[test]
    fn highlight_cap_refuses_replacement_and_reports_recovery_after_removal() {
        let mut editor = ChromascopeRadiantEditor::new(Arc::new(ChromascopeShared::new(
            crate::shared::DeviceKind::Viewer,
        )));
        for id in 30_000..30_000 + MAX_HIGHLIGHTS as u64 {
            editor.toggle_highlight(id, true);
        }
        editor.toggle_highlight(31_000, true);
        assert_eq!(editor.highlighted.len(), MAX_HIGHLIGHTS);
        assert!(editor.highlight_limit_reached);
        assert!(!editor.highlighted.iter().any(|source| source.id == 31_000));

        editor.toggle_highlight(30_000, true);
        assert_eq!(editor.highlighted.len(), MAX_HIGHLIGHTS - 1);
        assert!(!editor.highlight_limit_reached);
        editor.toggle_highlight(31_000, true);
        assert!(editor.highlighted.iter().any(|source| source.id == 31_000));
    }

    #[test]
    fn highlighted_source_uses_one_matching_tint_in_picker_and_scope() {
        let size = Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32);
        let layout = editor_layout(size);
        let companions = [source(7, true, Some(frame(-24.0)))];
        let mut highlighted = Vec::new();
        crate::highlight::toggle_highlight(&mut highlighted, 7, true);
        let expected = to_color(highlighted[0].color());
        let mut presentation = PresentationState::default();
        let mut plan = SurfacePaintPlan {
            clear_color: BACKGROUND,
            primitives: Vec::new(),
        };

        paint_editor_with_presentation(
            &mut plan,
            layout,
            Some(frame(-12.0)),
            &companions,
            &[7],
            &highlighted,
            &[],
            0.0,
            &mut presentation,
            PRESENTATION_FRAME_SECONDS,
            0.0,
            false,
        );

        assert!(plan.primitives.iter().any(|primitive| {
            matches!(
                primitive,
                PaintPrimitive::StrokePolyline(polyline)
                    if polyline.color == expected
                        && polyline.width == COMPANION_TRACE_THICKNESS
            )
        }));
        assert!(plan.primitives.iter().any(|primitive| {
            matches!(
                primitive,
                PaintPrimitive::Text(text)
                    if text.text.as_str() == "COMPANION 7" && text.color == expected
            )
        }));
    }

    #[test]
    fn stale_highlights_are_removed_when_a_companion_leaves_the_registry() {
        let mut editor = ChromascopeRadiantEditor::new(Arc::new(ChromascopeShared::new(
            crate::shared::DeviceKind::Viewer,
        )));
        editor.toggle_highlight(41, true);
        editor.toggle_highlight(42, true);
        editor.highlight_limit_reached = true;
        editor.prune_stale_highlights(&[source(41, false, None), source(42, true, None)]);

        assert_eq!(editor.highlighted.len(), 1);
        assert_eq!(editor.highlighted[0].id, 42);
        assert!(!editor.highlight_limit_reached);
    }

    #[test]
    fn maximum_companion_list_fits_source_panel() {
        let layout = editor_layout(Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32));
        let offset = max_source_scroll(layout.sources, MAX_COMPANIONS);
        let last_row = source_row_rect(layout.sources, MAX_COMPANIONS - 1, offset);
        assert!(last_row.min.y >= source_list_rect(layout.sources).min.y);
        assert!(
            last_row.max.y <= source_list_rect(layout.sources).max.y,
            "last row bottom {} exceeds list bottom {} (offset {})",
            last_row.max.y,
            source_list_rect(layout.sources).max.y,
            offset
        );
    }

    #[test]
    fn large_source_lists_materialize_only_the_visible_rows() {
        let layout = editor_layout(Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32));
        let list = source_list_rect(layout.sources);
        let visible = visible_source_range(MAX_COMPANIONS, list.height(), 0.0);

        assert!(visible.len() < MAX_COMPANIONS);
        assert_eq!(visible.start, 0);
        assert!(visible.end <= MAX_COMPANIONS);
        assert!(source_content_height(MAX_COMPANIONS) > list.height());
    }

    #[test]
    fn source_scroll_offset_reaches_the_last_companion_without_overflow() {
        let layout = editor_layout(Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32));
        let offset = max_source_scroll(layout.sources, MAX_COMPANIONS);
        let visible = visible_source_range(
            MAX_COMPANIONS,
            source_list_rect(layout.sources).height(),
            offset,
        );

        assert_eq!(visible.end, MAX_COMPANIONS);
        assert!(
            source_row_rect(layout.sources, MAX_COMPANIONS - 1, offset)
                .max
                .y
                <= source_list_rect(layout.sources).max.y,
            "last row bottom {} exceeds list bottom {} (offset {})",
            source_row_rect(layout.sources, MAX_COMPANIONS - 1, offset)
                .max
                .y,
            source_list_rect(layout.sources).max.y,
            offset
        );
    }

    #[test]
    fn selection_supports_the_full_bounded_companion_capacity() {
        let mut editor = ChromascopeRadiantEditor::new(Arc::new(ChromascopeShared::new(
            crate::shared::DeviceKind::Viewer,
        )));
        for id in 0..MAX_COMPANIONS as u64 {
            editor.toggle_companion(id + 20_000, true);
        }
        editor.toggle_companion(30_000, true);

        assert_eq!(editor.selected_ids.len(), MAX_COMPANIONS);
        assert!(editor.selected_ids.contains(&20_000));
        assert!(
            editor
                .selected_ids
                .contains(&(20_000 + MAX_COMPANIONS as u64 - 1))
        );
        assert!(!editor.selected_ids.contains(&30_000));
    }

    #[test]
    fn default_editor_is_wide_and_keeps_source_controls_readable() {
        assert_eq!(WINDOW_WIDTH * 4, WINDOW_HEIGHT * 9);
        let layout = editor_layout(Vector2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32));
        assert!(layout.graph.width() >= layout.sources.width() * 2.0);
        assert!(layout.sources.width() >= SOURCE_PANEL_MIN_WIDTH);
    }

    #[test]
    fn inactive_source_cannot_be_selected() {
        let mut selected = ChromascopeRadiantEditor::new(Arc::new(ChromascopeShared::new(
            crate::shared::DeviceKind::Viewer,
        )));
        selected.toggle_companion(4, false);
        assert!(selected.selected_ids.is_empty());
    }

    #[test]
    fn unrelated_event_does_not_change_selection() {
        let mut editor = ChromascopeRadiantEditor::new(Arc::new(ChromascopeShared::new(
            crate::shared::DeviceKind::Viewer,
        )));
        editor.dispatch_event(Event::PointerModifiersChanged {
            modifiers: PointerModifiers::default(),
        });
        assert!(editor.selected_ids.is_empty());
    }
}
