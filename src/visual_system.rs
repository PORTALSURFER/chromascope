//! Chromascope's Pump-aligned visual system.
//!
//! The palette intentionally mirrors Pump's canonical Radiant dark-coral
//! tokens.  Chromascope keeps its source-specific companion colors on top of
//! that neutral surface so multi-source traces remain easy to distinguish.

use crate::constants::Rgb;
use toybox::gui::Color;
use toybox::gui::declarative::{
    ColorTokens, ControlTokens, SpacingTokens, ThemeTokens, TypographyTokens,
};

/// Pump's canonical dark-coral palette, represented in format-neutral RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PumpAlignedPalette {
    /// Canvas and primary surface fill.
    pub(crate) canvas: Rgb,
    /// Raised/secondary surface fill.
    pub(crate) surface: Rgb,
    /// Overlay fill for selected or emphasized controls.
    pub(crate) overlay: Rgb,
    /// Standard border.
    pub(crate) border: Rgb,
    /// Strong border and divider.
    pub(crate) border_emphasis: Rgb,
    /// Primary grid line.
    pub(crate) grid_strong: Rgb,
    /// Secondary grid line and recessed track.
    pub(crate) grid_soft: Rgb,
    /// Primary readable text.
    pub(crate) text_primary: Rgb,
    /// Muted text and metadata.
    pub(crate) text_muted: Rgb,
    /// Pump's primary coral accent (`accent_mint` in Radiant).
    pub(crate) accent_primary: Rgb,
    /// Pump's secondary coral accent (`accent_copper` in Radiant).
    pub(crate) accent_secondary: Rgb,
    /// Warning state.
    pub(crate) warning: Rgb,
    /// Error/hot state.
    pub(crate) danger: Rgb,
    /// Disabled control fill.
    pub(crate) disabled_fill: Rgb,
}

/// Exact Pump palette values used by both Chromascope editor paths.
pub(crate) const PUMP_PALETTE: PumpAlignedPalette = PumpAlignedPalette {
    canvas: Rgb::new(27, 30, 30),
    surface: Rgb::new(27, 30, 30),
    overlay: Rgb::new(42, 45, 45),
    border: Rgb::new(58, 61, 61),
    border_emphasis: Rgb::new(64, 67, 66),
    grid_strong: Rgb::new(54, 57, 57),
    grid_soft: Rgb::new(40, 43, 43),
    text_primary: Rgb::new(216, 215, 211),
    text_muted: Rgb::new(153, 155, 154),
    accent_primary: Rgb::new(233, 88, 67),
    accent_secondary: Rgb::new(241, 108, 86),
    warning: Rgb::new(217, 151, 95),
    danger: Rgb::new(239, 76, 61),
    disabled_fill: Rgb::new(36, 40, 41),
};

/// Geometry shared with Pump's compact editor language.
#[cfg(all(feature = "vst3", target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PumpAlignedMetrics {
    /// Base spacing unit.
    pub(crate) base: f32,
    /// Four-pixel spacing role.
    pub(crate) space_4: f32,
    /// Eight-pixel spacing role.
    pub(crate) space_8: f32,
    /// Twelve-pixel spacing role.
    pub(crate) space_12: f32,
    /// Sixteen-pixel spacing role.
    pub(crate) space_16: f32,
    /// Surface padding.
    pub(crate) padding: f32,
    /// Control/panel gap.
    pub(crate) gap: f32,
    /// Rounded surface radius.
    pub(crate) radius: f32,
    /// One-pixel border width.
    pub(crate) border: f32,
    /// Compact control height.
    pub(crate) control_height: f32,
    /// Compact source-row gap.
    pub(crate) row_gap: f32,
}

/// Exact Pump geometry roles used by Chromascope's native composition.
#[cfg(all(feature = "vst3", target_os = "macos"))]
pub(crate) const PUMP_ALIGNED_METRICS: PumpAlignedMetrics = PumpAlignedMetrics {
    base: 3.4,
    space_4: 3.4,
    space_8: 6.8,
    space_12: 10.2,
    space_16: 13.6,
    padding: 10.2,
    gap: 6.8,
    radius: 6.8,
    border: 1.0,
    control_height: 27.2,
    row_gap: 1.7,
};

/// Pump's typography roles in logical pixels.
#[cfg(all(feature = "vst3", target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PumpAlignedTypography {
    /// Brand/title size and line height.
    pub(crate) brand: (f32, f32),
    /// Body size and line height.
    pub(crate) body: (f32, f32),
    /// Value size and line height.
    pub(crate) value: (f32, f32),
    /// Compact control-label size and line height.
    pub(crate) control_label: (f32, f32),
    /// Metadata size and line height.
    pub(crate) meta: (f32, f32),
}

/// Exact Pump typography roles used by Chromascope's native composition.
#[cfg(all(feature = "vst3", target_os = "macos"))]
pub(crate) const PUMP_ALIGNED_TYPOGRAPHY: PumpAlignedTypography = PumpAlignedTypography {
    brand: (18.7, 23.8),
    body: (11.9, 15.3),
    value: (10.2, 13.6),
    control_label: (8.5, 13.6),
    meta: (8.0, 11.9),
};

/// Convert one format-neutral palette value for the declarative renderer.
pub(crate) fn to_gui_color(rgb: Rgb) -> Color {
    Color::rgb(rgb.red, rgb.green, rgb.blue)
}

/// Build the declarative token set with Pump's surface, text, and accent roles.
pub(crate) fn pump_aligned_theme_tokens() -> ThemeTokens {
    ThemeTokens {
        colors: ColorTokens {
            background: to_gui_color(PUMP_PALETTE.canvas),
            surface: to_gui_color(PUMP_PALETTE.surface),
            border: to_gui_color(PUMP_PALETTE.border),
            text: to_gui_color(PUMP_PALETTE.text_primary),
            accent: to_gui_color(PUMP_PALETTE.accent_primary),
        },
        typography: TypographyTokens { text_scale: 2 },
        // Patchbay's spacing tokens are integer-valued; these are the nearest
        // compact representations of Pump's 3.4/6.8/10.2/13.6 scale.
        spacing: SpacingTokens {
            xs: 3,
            sm: 7,
            md: 10,
            lg: 14,
        },
        controls: ControlTokens {
            knob_diameter: 48,
            slider_width: 180,
            slider_height: 27,
            toggle_width: 28,
            toggle_height: 27,
            button_width: 82,
            button_height: 27,
            dropdown_width: 82,
            dropdown_height: 27,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matches_pumps_canonical_dark_coral_values() {
        assert_eq!(PUMP_PALETTE.canvas, Rgb::new(27, 30, 30));
        assert_eq!(PUMP_PALETTE.overlay, Rgb::new(42, 45, 45));
        assert_eq!(PUMP_PALETTE.grid_strong, Rgb::new(54, 57, 57));
        assert_eq!(PUMP_PALETTE.accent_primary, Rgb::new(233, 88, 67));
        assert_eq!(PUMP_PALETTE.accent_secondary, Rgb::new(241, 108, 86));
        assert_eq!(PUMP_PALETTE.text_primary, Rgb::new(216, 215, 211));
    }

    #[cfg(all(feature = "vst3", target_os = "macos"))]
    #[test]
    fn metrics_and_typography_match_pumps_compact_roles() {
        assert_eq!(PUMP_ALIGNED_METRICS.base, 3.4);
        assert_eq!(PUMP_ALIGNED_METRICS.padding, 10.2);
        assert_eq!(PUMP_ALIGNED_METRICS.radius, 6.8);
        assert_eq!(PUMP_ALIGNED_METRICS.control_height, 27.2);
        assert_eq!(PUMP_ALIGNED_TYPOGRAPHY.body, (11.9, 15.3));
        assert_eq!(PUMP_ALIGNED_TYPOGRAPHY.meta, (8.0, 11.9));
    }

    #[test]
    fn declarative_tokens_use_pump_roles() {
        let tokens = pump_aligned_theme_tokens();
        assert_eq!(tokens.colors.background, to_gui_color(PUMP_PALETTE.canvas));
        assert_eq!(tokens.colors.surface, to_gui_color(PUMP_PALETTE.surface));
        assert_eq!(
            tokens.colors.accent,
            to_gui_color(PUMP_PALETTE.accent_primary)
        );
        assert_eq!(tokens.controls.toggle_height, 27);
    }
}
