//! Compose paint layers without changing source text or syntax foregrounds.

use gpui_kit::{HighlightStyle, Hsla, UnderlineStyle, px};
use std::ops::Range;

pub(super) struct OverlayColors {
    pub changed: Hsla,
    pub selected: Hsla,
    pub foreground: Hsla,
}

pub(super) fn compose(
    text_len: usize,
    syntax: &[(Range<usize>, HighlightStyle)],
    changed: &[Range<usize>],
    selected: Option<&Range<usize>>,
    marked: Option<&Range<usize>>,
    colors: &OverlayColors,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut boundaries = vec![0, text_len];
    for (range, _) in syntax {
        boundaries.extend([range.start, range.end]);
    }
    for range in changed.iter().chain(selected).chain(marked) {
        boundaries.extend([range.start, range.end]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
        .windows(2)
        .filter_map(|pair| {
            let range = pair[0]..pair[1];
            if range.is_empty() {
                return None;
            }
            let mut style = syntax
                .iter()
                .find(|(span, _)| span.start <= range.start && span.end >= range.end)
                .map(|(_, style)| *style)
                .unwrap_or_default();
            let overlaps = |span: &Range<usize>| span.start < range.end && span.end > range.start;
            if changed.iter().any(overlaps) {
                style.background_color = Some(colors.changed);
            }
            // A text selection must remain unambiguous on top of diff emphasis.
            if selected.is_some_and(overlaps) {
                style.background_color = Some(colors.selected);
            }
            if marked.is_some_and(overlaps) {
                style.underline = Some(UnderlineStyle {
                    color: Some(colors.foreground),
                    thickness: px(1.0),
                    wavy: false,
                });
            }
            Some((range, style))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{OverlayColors, compose};
    use gpui_kit::{HighlightStyle, hsla};

    #[test]
    fn selection_wins_over_diff_while_syntax_and_composition_remain_visible() {
        let foreground = hsla(0.6, 0.5, 0.8, 1.0);
        let colors = OverlayColors {
            changed: hsla(0.1, 0.5, 0.3, 1.0),
            selected: hsla(0.6, 0.5, 0.3, 1.0),
            foreground,
        };
        let syntax = vec![(
            0..6,
            HighlightStyle {
                color: Some(foreground),
                ..HighlightStyle::default()
            },
        )];
        let changes = vec![1..5, 7..8];
        let runs = compose(8, &syntax, &changes, Some(&(2..4)), Some(&(3..5)), &colors);
        for byte in 0..8 {
            let (_, style) = runs
                .iter()
                .find(|(range, _)| range.contains(&byte))
                .unwrap();
            assert_eq!(style.color, (byte < 6).then_some(foreground));
            assert_eq!(
                style.background_color,
                if (2..4).contains(&byte) {
                    Some(colors.selected)
                } else if (1..5).contains(&byte) || byte == 7 {
                    Some(colors.changed)
                } else {
                    None
                }
            );
            assert_eq!(style.underline.is_some(), (3..5).contains(&byte));
        }
        assert_eq!(runs.first().unwrap().0.start, 0);
        assert_eq!(runs.last().unwrap().0.end, 8);
        assert!(runs.windows(2).all(|pair| pair[0].0.end == pair[1].0.start));
    }
}
