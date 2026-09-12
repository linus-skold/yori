//! Immutable display meaning built alongside presentation-only row insertion.

use std::ops::Range;

use super::{ConflictId, MergeRow, MergeSession};

#[derive(Default)]
pub(in crate::editor) struct MergeDisplay {
    rows: Vec<DisplayRow>,
    conflicts: Vec<ConflictDisplay>,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::editor) struct DisplayRow {
    pub kind: RowKind,
    pub sources: MergeRow,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::editor) enum RowKind {
    // Alignment gaps remain ordinary rows, even when a pane has no source.
    Aligned,
    ConflictHeader(ConflictId),
    Base(BaseRow),
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::editor) enum BaseRow {
    Caption,
    EmptyAncestor,
    SourceLine(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::editor) struct ConflictDisplay {
    pub id: ConflictId,
    pub header_row: usize,
    // Translated projection endpoints, including any intervening chrome.
    // Empty spans stay empty; this is not the header-inclusive control extent.
    pub source_span: Range<usize>,
    pub control_span: Range<usize>,
    pub base_caption: Option<usize>,
}

impl MergeDisplay {
    pub fn build(session: &MergeSession, expanded_base: Option<ConflictId>) -> Self {
        let projection = session.alignment();
        let spans: Vec<_> = session
            .conflicts()
            .iter()
            .map(|conflict| {
                projection
                    .conflict_rows(conflict.id)
                    .expect("known conflict")
            })
            .collect();
        let mut starts: Vec<_> = spans
            .iter()
            .enumerate()
            .map(|(index, rows)| (rows.start, ConflictId(index)))
            .collect();
        starts.sort_unstable_by_key(|&(row, id)| (row, id.0));
        let mut starts = starts.into_iter().peekable();

        let mut display = Self {
            rows: Vec::new(),
            conflicts: session
                .conflicts()
                .iter()
                .map(|conflict| ConflictDisplay {
                    id: conflict.id,
                    header_row: 0,
                    source_span: 0..0,
                    control_span: 0..0,
                    base_caption: None,
                })
                .collect(),
        };
        let mut mapped = Vec::with_capacity(projection.rows().len() + 1);

        // Insert controls before source rows, then map both ends of each span.
        // No header or ancestor excerpt can acquire a participating source byte.
        for boundary in 0..=projection.rows().len() {
            while let Some(&(start, id)) = starts.peek() {
                if start != boundary {
                    break;
                }

                starts.next();
                display.conflicts[id.0].header_row = display.rows.len();
                display.push_chrome(RowKind::ConflictHeader(id));
                if expanded_base == Some(id) {
                    display.conflicts[id.0].base_caption = Some(display.rows.len());
                    display.insert_base_preview(session, id);
                }
            }

            mapped.push(display.rows.len());
            if let Some(row) = projection.rows().get(boundary) {
                display.rows.push(DisplayRow {
                    kind: RowKind::Aligned,
                    sources: row.clone(),
                });
            }
        }

        for (conflict, span) in display.conflicts.iter_mut().zip(spans) {
            let end = if span.is_empty() {
                mapped[span.start]
            } else {
                mapped[span.end - 1] + 1
            };
            conflict.source_span = mapped[span.start]..end;
            conflict.control_span = conflict.header_row..end;
        }

        display
    }

    pub fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }

    // Ordered and indexable by ConflictId, independently of header insertion order.
    pub fn conflicts(&self) -> &[ConflictDisplay] {
        &self.conflicts
    }

    fn insert_base_preview(&mut self, session: &MergeSession, id: ConflictId) {
        let bytes = &session.conflicts()[id.0].base;
        let base = session.base();
        let first = base
            .lines()
            .partition_point(|line| line.full.end <= bytes.start);
        let last = if bytes.is_empty() {
            first
        } else {
            base.line_at_offset(bytes.end - 1) + 1
        };

        self.push_chrome(RowKind::Base(BaseRow::Caption));
        if first == last {
            self.push_chrome(RowKind::Base(BaseRow::EmptyAncestor));
        } else {
            for line in first..last {
                self.push_chrome(RowKind::Base(BaseRow::SourceLine(line)));
            }
        }
    }

    fn push_chrome(&mut self, kind: RowKind) {
        self.rows.push(DisplayRow {
            kind,
            sources: MergeRow {
                local: None,
                result: None,
                incoming: None,
            },
        });
    }
}

#[cfg(test)]
mod tests;
