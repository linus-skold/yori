//! Presentation-only tab expansion and aligned hit mapping.

use std::ops::Range;
use yori_diff::Alignment;
use yori_document::Document;

/// Maps an aligned row and shaped display byte to an authoritative source offset.
/// A row below the complete alignment is EOF, making a final line terminator selectable.
#[must_use]
pub fn source_offset_at(
    alignment: &Alignment,
    document: &Document,
    row: usize,
    left_side: bool,
    display_byte: usize,
    tab_width: usize,
) -> usize {
    let Some(alignment_row) = alignment.rows().get(row) else {
        return document.text().len();
    };

    let line = if left_side {
        alignment_row.left
    } else {
        alignment_row.right
    };
    let Some(line) = line else {
        return alignment.gap_offset(document, row, left_side);
    };

    let source_line = &document.lines()[line];
    DisplayLine::from_source(document.content(line), source_line.content.start, tab_width)
        .source_offset(display_byte)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayLine {
    pub text: String,
    columns: usize,
    // Character boundaries in display bytes mapped to authoritative source bytes.
    boundaries: Vec<(usize, usize)>,
    // One span per source character; tabs own their complete visual expansion.
    spans: Vec<(Range<usize>, Range<usize>)>,
}

impl DisplayLine {
    #[must_use]
    pub fn from_source(text: &str, source_start: usize, tab_width: usize) -> Self {
        let mut display = String::new();
        let mut boundaries = vec![(0, source_start)];
        let mut spans = Vec::new();
        let mut column = 0;

        for (relative, character) in text.char_indices() {
            let source = source_start + relative;
            let display_start = display.len();

            if character == '\t' {
                let width = tab_width - (column % tab_width);
                for step in 0..width {
                    display.push(' ');
                    let mapped = if (step + 1) * 2 <= width {
                        source
                    } else {
                        source + 1
                    };
                    boundaries.push((display.len(), mapped));
                }
                column += width;
            } else {
                display.push(character);
                column += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
                boundaries.push((display.len(), source + character.len_utf8()));
            }

            spans.push((
                source..source + character.len_utf8(),
                display_start..display.len(),
            ));
        }

        Self {
            text: display,
            columns: column,
            boundaries,
            spans,
        }
    }

    #[must_use]
    pub fn columns(&self) -> usize {
        self.columns
    }

    #[must_use]
    pub fn source_offset(&self, display_byte: usize) -> usize {
        match self
            .boundaries
            .binary_search_by_key(&display_byte, |&(display, _)| display)
        {
            Ok(index) => self.boundaries[index].1,
            Err(index) => self.boundaries[index.saturating_sub(1)].1,
        }
    }

    #[must_use]
    pub fn display_offset(&self, source: usize) -> usize {
        self.spans
            .iter()
            .find(|(span, _)| source < span.end)
            .map_or(self.text.len(), |(_, display)| display.start)
    }

    #[must_use]
    pub fn display_range(&self, source: Range<usize>) -> Range<usize> {
        let mut overlapping = self.spans.iter().filter(|(source_span, _)| {
            source_span.start < source.end && source_span.end > source.start
        });
        let Some((_, first)) = overlapping.next() else {
            return 0..0;
        };

        let mut range = first.clone();
        for (_, display_span) in overlapping {
            range.end = display_span.end;
        }

        range
    }
}

#[must_use]
pub fn max_display_columns(document: &Document, tab_width: usize) -> usize {
    document
        .lines()
        .iter()
        .map(|line| {
            document
                .copy_range(line.content.clone())
                .chars()
                .fold(0, |column, ch| {
                    column
                        + if ch == '\t' {
                            tab_width - column % tab_width
                        } else {
                            unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0)
                        }
                })
        })
        .max()
        .unwrap_or(0)
}
