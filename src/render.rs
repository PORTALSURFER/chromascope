//! Spectrum graph drawing commands for the Toybox declarative surface.

use crate::analysis::{SpectrumFrame, band_center_frequency_hz};
use crate::constants::{
    DISPLAY_TRACE_SAMPLES, MAIN_SPECTRUM_COLOR, MAX_BANDS, MAX_LEVEL_DB, MIN_FREQUENCY_HZ,
    MIN_LEVEL_DB, Rgb,
};
use crate::highlight::{HighlightedSource, color_for};
use crate::registry::CompanionSourceSnapshot;
use crate::visual_system::PUMP_PALETTE;
use toybox::gui::declarative::SurfaceCommand;
use toybox::gui::{Color, Point, Rect, Size};

const BACKGROUND: Color = Color::rgb(
    PUMP_PALETTE.canvas.red,
    PUMP_PALETTE.canvas.green,
    PUMP_PALETTE.canvas.blue,
);
const BORDER: Color = Color::rgb(
    PUMP_PALETTE.border.red,
    PUMP_PALETTE.border.green,
    PUMP_PALETTE.border.blue,
);
const MAJOR_GRID: Color = Color::rgba(
    PUMP_PALETTE.grid_strong.red,
    PUMP_PALETTE.grid_strong.green,
    PUMP_PALETTE.grid_strong.blue,
    170,
);
const MINOR_GRID: Color = Color::rgba(
    PUMP_PALETTE.grid_soft.red,
    PUMP_PALETTE.grid_soft.green,
    PUMP_PALETTE.grid_soft.blue,
    135,
);
const LABEL: Color = Color::rgb(
    PUMP_PALETTE.text_muted.red,
    PUMP_PALETTE.text_muted.green,
    PUMP_PALETTE.text_muted.blue,
);
const MAIN_TRACE_THICKNESS: f32 = 1.7;
const COMPANION_TRACE_THICKNESS: f32 = 1.275;
const PLOT_PADDING_LEFT: i32 = 42;
const PLOT_PADDING_RIGHT: i32 = 12;
const PLOT_PADDING_TOP: i32 = 12;
const PLOT_PADDING_BOTTOM: i32 = 28;

/// Return a display-domain band position for one dense trace sample.
pub(crate) fn display_trace_band_position(sample: usize) -> f32 {
    let sample = sample.min(DISPLAY_TRACE_SAMPLES.saturating_sub(1));
    let last_sample = DISPLAY_TRACE_SAMPLES.saturating_sub(1).max(1) as f32;
    sample as f32 * MAX_BANDS.saturating_sub(1) as f32 / last_sample
}

/// Interpolate one measured spectrum value for display without overshooting a
/// neighboring peak or valley.
///
/// This is a clamped cubic Hermite interpolation in logarithmic-band index
/// space. The analyzer's bands are logarithmically spaced, so this is also the
/// display frequency domain. Clamping each segment preserves transient peaks
/// and avoids ringing around steep spectral changes.
pub(crate) fn interpolated_band_value(values: &[f32; MAX_BANDS], position: f32) -> f32 {
    let last_band = MAX_BANDS.saturating_sub(1);
    let position = position.clamp(0.0, last_band as f32);
    let lower = position.floor() as usize;
    if lower >= last_band {
        return finite_display_value(values[last_band]);
    }

    let upper = lower + 1;
    let t = position - lower as f32;
    let y0 = finite_display_value(values[lower]);
    let y1 = finite_display_value(values[upper]);
    let previous = finite_display_value(values[lower.saturating_sub(1)]);
    let next = finite_display_value(values[(upper + 1).min(last_band)]);
    let tangent0 = if lower == 0 {
        y1 - y0
    } else {
        (y1 - previous) * 0.5
    };
    let tangent1 = if upper == last_band {
        y1 - y0
    } else {
        (next - y0) * 0.5
    };
    let t2 = t * t;
    let t3 = t2 * t;
    let interpolated = (2.0 * t3 - 3.0 * t2 + 1.0) * y0
        + (t3 - 2.0 * t2 + t) * tangent0
        + (-2.0 * t3 + 3.0 * t2) * y1
        + (t3 - t2) * tangent1;
    interpolated.clamp(y0.min(y1), y0.max(y1))
}

fn finite_display_value(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        MIN_LEVEL_DB
    }
}

/// Build the graph surface for the viewer's main input and selected sources.
pub fn build_spectrum_surface_commands(
    main: Option<SpectrumFrame>,
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
    size: Size,
) -> Vec<SurfaceCommand> {
    build_spectrum_surface_commands_with_highlights(main, companions, selected_ids, &[], size)
}

/// Build the graph surface with independent source-highlight colors.
pub(crate) fn build_spectrum_surface_commands_with_highlights(
    main: Option<SpectrumFrame>,
    companions: &[CompanionSourceSnapshot],
    selected_ids: &[u64],
    highlighted: &[HighlightedSource],
    size: Size,
) -> Vec<SurfaceCommand> {
    let width = size.width.max(1);
    let height = size.height.max(1);
    let plot = plot_rect(width, height);
    let mut commands = Vec::with_capacity(32 + DISPLAY_TRACE_SAMPLES * (selected_ids.len() + 1));

    commands.push(SurfaceCommand::FillRect {
        rect: Rect {
            origin: Point { x: 0, y: 0 },
            size: Size { width, height },
        },
        color: BACKGROUND,
    });
    commands.push(SurfaceCommand::StrokeRect {
        rect: plot,
        thickness: 1,
        color: BORDER,
    });
    draw_grid(&mut commands, plot);

    for companion in companions {
        if companion.active
            && selected_ids.contains(&companion.id)
            && let Some(frame) = companion.frame
        {
            commands.push(SurfaceCommand::Polyline {
                points: trace_points(&frame, plot),
                thickness: COMPANION_TRACE_THICKNESS,
                color: to_color(color_for(highlighted, companion.id).unwrap_or(companion.color)),
            });
        }
    }

    if let Some(frame) = main {
        commands.push(SurfaceCommand::Polyline {
            points: trace_points(&frame, plot),
            thickness: MAIN_TRACE_THICKNESS,
            color: to_color(MAIN_SPECTRUM_COLOR),
        });
    } else {
        commands.push(SurfaceCommand::Text {
            origin: Point {
                x: plot.origin.x + 16,
                y: plot.origin.y + plot.size.height as i32 / 2,
            },
            text: "WAITING FOR MAIN INPUT".to_string(),
            color: LABEL,
            scale: 1,
        });
    }

    commands
}

fn plot_rect(width: u32, height: u32) -> Rect {
    let origin_x = PLOT_PADDING_LEFT.min(width.saturating_sub(1) as i32);
    let origin_y = PLOT_PADDING_TOP.min(height.saturating_sub(1) as i32);
    let right_padding = PLOT_PADDING_RIGHT.max(0) as u32;
    let bottom_padding = PLOT_PADDING_BOTTOM.max(0) as u32;
    Rect {
        origin: Point {
            x: origin_x,
            y: origin_y,
        },
        size: Size {
            width: width
                .saturating_sub(origin_x.max(0) as u32)
                .saturating_sub(right_padding)
                .max(1),
            height: height
                .saturating_sub(origin_y.max(0) as u32)
                .saturating_sub(bottom_padding)
                .max(1),
        },
    }
}

fn draw_grid(commands: &mut Vec<SurfaceCommand>, plot: Rect) {
    let major_frequencies = [20.0, 100.0, 1_000.0, 10_000.0, 20_000.0];
    for frequency in [20.0, 100.0, 1_000.0, 10_000.0] {
        for multiplier in 2..10 {
            let minor_frequency = frequency * multiplier as f32;
            if minor_frequency < 20_000.0 {
                let x = frequency_to_x(minor_frequency, plot);
                commands.push(SurfaceCommand::Line {
                    start: Point {
                        x,
                        y: plot.origin.y,
                    },
                    end: Point {
                        x,
                        y: plot.origin.y + plot.size.height as i32,
                    },
                    color: MINOR_GRID,
                });
            }
        }
    }

    for frequency in major_frequencies {
        let x = frequency_to_x(frequency, plot);
        commands.push(SurfaceCommand::Line {
            start: Point {
                x,
                y: plot.origin.y,
            },
            end: Point {
                x,
                y: plot.origin.y + plot.size.height as i32,
            },
            color: MAJOR_GRID,
        });
        commands.push(SurfaceCommand::Text {
            origin: Point {
                x: x.saturating_sub(12),
                y: plot.origin.y + plot.size.height as i32 + 16,
            },
            text: frequency_label(frequency).to_string(),
            color: LABEL,
            scale: 1,
        });
    }

    for level in (-100..=0).step_by(20) {
        let y = level_to_y(level as f32, plot);
        commands.push(SurfaceCommand::Line {
            start: Point {
                x: plot.origin.x,
                y,
            },
            end: Point {
                x: plot.origin.x + plot.size.width as i32,
                y,
            },
            color: MAJOR_GRID,
        });
        commands.push(SurfaceCommand::Text {
            origin: Point {
                x: 4,
                y: y.saturating_sub(5),
            },
            text: format!("{level}"),
            color: LABEL,
            scale: 1,
        });
    }
}

fn trace_points(frame: &SpectrumFrame, plot: Rect) -> Vec<Point> {
    let low_frequency = band_center_frequency_hz(0, 48_000.0);
    let high_frequency = band_center_frequency_hz(MAX_BANDS - 1, 48_000.0);
    (0..DISPLAY_TRACE_SAMPLES)
        .map(|sample| {
            let position = display_trace_band_position(sample);
            let frequency = low_frequency
                * (high_frequency / low_frequency)
                    .powf(position / MAX_BANDS.saturating_sub(1).max(1) as f32);
            let x = frequency_to_x(frequency, plot);
            let y = level_to_y(interpolated_band_value(&frame.values, position), plot);
            Point { x, y }
        })
        .collect()
}

fn frequency_to_x(frequency: f32, plot: Rect) -> i32 {
    let minimum = MIN_FREQUENCY_HZ.log10();
    let maximum = 20_000.0f32.log10();
    let ratio = ((frequency.clamp(MIN_FREQUENCY_HZ, 20_000.0).log10() - minimum)
        / (maximum - minimum))
        .clamp(0.0, 1.0);
    plot.origin.x + (ratio * plot.size.width.saturating_sub(1) as f32).round() as i32
}

fn level_to_y(level: f32, plot: Rect) -> i32 {
    let ratio = ((MAX_LEVEL_DB - level.clamp(MIN_LEVEL_DB, MAX_LEVEL_DB))
        / (MAX_LEVEL_DB - MIN_LEVEL_DB))
        .clamp(0.0, 1.0);
    plot.origin.y + (ratio * plot.size.height.saturating_sub(1) as f32).round() as i32
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

fn to_color(rgb: Rgb) -> Color {
    Color::rgb(rgb.red, rgb.green, rgb.blue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::MAX_BANDS;

    fn frame(value: f32) -> SpectrumFrame {
        SpectrumFrame {
            sequence: 1,
            sample_position: None,
            values: [value; MAX_BANDS],
        }
    }

    fn source(
        id: u64,
        color: Rgb,
        active: bool,
        frame: Option<SpectrumFrame>,
    ) -> CompanionSourceSnapshot {
        CompanionSourceSnapshot {
            id,
            name: format!("COMPANION {id}"),
            color,
            active,
            analysis_requested: active && frame.is_some(),
            frame,
        }
    }

    #[test]
    fn graph_always_draws_main_trace_when_available() {
        let commands = build_spectrum_surface_commands(
            Some(frame(-12.0)),
            &[],
            &[],
            Size {
                width: 800,
                height: 400,
            },
        );
        assert!(commands.iter().any(|command| {
            matches!(command, SurfaceCommand::Polyline { color, thickness, .. } if *color == to_color(MAIN_SPECTRUM_COLOR) && *thickness == MAIN_TRACE_THICKNESS)
        }));
    }

    #[test]
    fn graph_only_draws_selected_active_companion_traces() {
        let selected_color = Rgb::new(240, 80, 100);
        let unselected_color = Rgb::new(80, 240, 100);
        let commands = build_spectrum_surface_commands(
            None,
            &[
                source(1, selected_color, true, Some(frame(-20.0))),
                source(2, unselected_color, true, Some(frame(-30.0))),
                source(3, Rgb::new(100, 100, 240), false, Some(frame(-40.0))),
            ],
            &[1, 3],
            Size {
                width: 800,
                height: 400,
            },
        );
        let companion_colors: Vec<Color> = commands
            .iter()
            .filter_map(|command| match command {
                SurfaceCommand::Polyline {
                    color, thickness, ..
                } if *thickness == COMPANION_TRACE_THICKNESS => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(companion_colors, vec![to_color(selected_color)]);
    }

    #[test]
    fn highlighted_companion_trace_uses_the_same_near_white_tint_as_picker_state() {
        let source_color = Rgb::new(80, 240, 100);
        let mut highlighted = Vec::new();
        crate::highlight::toggle_highlight(&mut highlighted, 2, true);
        let commands = build_spectrum_surface_commands_with_highlights(
            None,
            &[source(2, source_color, true, Some(frame(-30.0)))],
            &[2],
            &highlighted,
            Size {
                width: 800,
                height: 400,
            },
        );
        let colors: Vec<Color> = commands
            .iter()
            .filter_map(|command| match command {
                SurfaceCommand::Polyline {
                    color, thickness, ..
                } if *thickness == COMPANION_TRACE_THICKNESS => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(colors, vec![to_color(highlighted[0].color())]);
        assert_ne!(colors, vec![to_color(source_color)]);
    }

    #[test]
    fn display_trace_is_dense_but_keeps_the_measured_band_endpoints() {
        let mut values = [MIN_LEVEL_DB; MAX_BANDS];
        values[0] = -80.0;
        values[1] = -20.0;
        values[2] = -60.0;
        let frame = SpectrumFrame {
            sequence: 1,
            sample_position: None,
            values,
        };
        let plot = plot_rect(800, 400);
        let points = trace_points(&frame, plot);

        assert_eq!(points.len(), DISPLAY_TRACE_SAMPLES);
        assert!(points.len() > MAX_BANDS);
        assert_eq!(
            points.first().expect("first display point").y,
            level_to_y(-80.0, plot)
        );
        assert_eq!(
            points.last().expect("last display point").y,
            level_to_y(MIN_LEVEL_DB, plot)
        );
    }

    #[test]
    fn display_interpolation_is_deterministic_and_does_not_ring() {
        let mut values = [0.0; MAX_BANDS];
        values[0] = -80.0;
        values[1] = -20.0;
        values[2] = -60.0;
        let midpoint = interpolated_band_value(&values, 1.5);
        let repeated = interpolated_band_value(&values, 1.5);

        assert_eq!(midpoint, repeated);
        assert!((-60.0..=-20.0).contains(&midpoint));
    }

    #[test]
    fn frequency_axis_is_logarithmic_between_20_hz_and_20_khz() {
        let plot = plot_rect(800, 400);
        let x100 = frequency_to_x(100.0, plot);
        let x200 = frequency_to_x(200.0, plot);
        let x400 = frequency_to_x(400.0, plot);
        let x1000 = frequency_to_x(1_000.0, plot);
        let x2000 = frequency_to_x(2_000.0, plot);
        let x4000 = frequency_to_x(4_000.0, plot);

        assert_eq!(frequency_to_x(20.0, plot), plot.origin.x);
        assert_eq!(
            frequency_to_x(20_000.0, plot),
            plot.origin.x + plot.size.width.saturating_sub(1) as i32
        );
        assert!((x200 - x100 - (x400 - x200)).abs() <= 1);
        assert!((x2000 - x1000 - (x4000 - x2000)).abs() <= 1);
    }
}
