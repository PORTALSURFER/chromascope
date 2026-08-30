//! UI-only source-highlight state and stable tint assignment.

use crate::constants::{HIGHLIGHT_COLORS, MAX_HIGHLIGHTS, Rgb};

/// One highlighted companion and its stable palette slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HighlightedSource {
    pub(crate) id: u64,
    color_index: usize,
}

impl HighlightedSource {
    /// Return the near-white tint assigned to this source.
    pub(crate) fn color(self) -> Rgb {
        HIGHLIGHT_COLORS
            .get(self.color_index)
            .copied()
            .unwrap_or(HIGHLIGHT_COLORS[0])
    }
}

/// Outcome of one Command-click highlight toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HighlightToggle {
    /// A new source was highlighted.
    Added,
    /// An existing source was unhighlighted.
    Removed,
    /// The source was already highlighted and was not changed.
    Ignored,
    /// The bounded highlight capacity was reached, so no source was replaced.
    AtCapacity,
}

/// Toggle one active source while keeping palette slots stable for survivors.
pub(crate) fn toggle_highlight(
    highlighted: &mut Vec<HighlightedSource>,
    id: u64,
    active: bool,
) -> HighlightToggle {
    if !active {
        return HighlightToggle::Ignored;
    }
    if let Some(index) = highlighted.iter().position(|source| source.id == id) {
        highlighted.remove(index);
        return HighlightToggle::Removed;
    }
    if highlighted.len() >= MAX_HIGHLIGHTS {
        return HighlightToggle::AtCapacity;
    }

    let color_index = (0..MAX_HIGHLIGHTS)
        .find(|index| {
            highlighted
                .iter()
                .all(|source| source.color_index != *index)
        })
        .expect("highlight capacity and palette must stay in sync");
    highlighted.push(HighlightedSource { id, color_index });
    HighlightToggle::Added
}

/// Return a highlighted source's tint, if its id is currently highlighted.
pub(crate) fn color_for(highlighted: &[HighlightedSource], id: u64) -> Option<Rgb> {
    highlighted
        .iter()
        .find(|source| source.id == id)
        .map(|source| source.color())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_slots_are_distinct_and_stable_until_removed() {
        let mut highlighted = Vec::new();
        for id in 10..10 + MAX_HIGHLIGHTS as u64 {
            assert_eq!(
                toggle_highlight(&mut highlighted, id, true),
                HighlightToggle::Added
            );
        }
        let colors: Vec<_> = highlighted.iter().map(|source| source.color()).collect();
        assert_eq!(colors.len(), MAX_HIGHLIGHTS);
        for (index, color) in colors.iter().enumerate() {
            assert_eq!(
                colors
                    .iter()
                    .filter(|candidate| **candidate == *color)
                    .count(),
                1
            );
            assert_eq!(color, &HIGHLIGHT_COLORS[index]);
        }
        let second_color = color_for(&highlighted, 11).expect("second tint");

        assert_eq!(
            toggle_highlight(&mut highlighted, 10, true),
            HighlightToggle::Removed
        );
        assert_eq!(color_for(&highlighted, 11), Some(second_color));
        assert_eq!(highlighted.len(), MAX_HIGHLIGHTS - 1);
        assert_eq!(
            toggle_highlight(&mut highlighted, 99, true),
            HighlightToggle::Added
        );
        assert_eq!(color_for(&highlighted, 99), Some(HIGHLIGHT_COLORS[0]));
    }

    #[test]
    fn capacity_refuses_a_new_source_without_replacing_existing_entries() {
        let mut highlighted = Vec::new();
        for id in 0..MAX_HIGHLIGHTS as u64 {
            assert_eq!(
                toggle_highlight(&mut highlighted, id, true),
                HighlightToggle::Added
            );
        }
        let before = highlighted.clone();
        assert_eq!(
            toggle_highlight(&mut highlighted, 99, true),
            HighlightToggle::AtCapacity
        );
        assert_eq!(highlighted, before);
    }

    #[test]
    fn inactive_sources_cannot_enter_the_highlight_set() {
        let mut highlighted = Vec::new();
        assert_eq!(
            toggle_highlight(&mut highlighted, 10, false),
            HighlightToggle::Ignored
        );
        assert!(highlighted.is_empty());
    }
}
