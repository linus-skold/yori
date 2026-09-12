//! Compose two comparisons around the same result document. Source rows remain
//! distinct from display rows, including gaps shared by both input panes.

use std::ops::Range;

use yori_document::Document;

use super::MergeSession;
use crate::Alignment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRow {
    pub local: Option<usize>,
    pub result: Option<usize>,
    pub incoming: Option<usize>,
}

#[derive(Debug)]
pub struct MergeAlignment {
    rows: Vec<MergeRow>,
    conflicts: Vec<Range<usize>>,
}

impl MergeAlignment {
    #[must_use]
    pub fn rows(&self) -> &[MergeRow] {
        &self.rows
    }

    #[must_use]
    pub fn conflict_rows(&self, id: super::ConflictId) -> Option<Range<usize>> {
        self.conflicts.get(id.0).cloned()
    }
}

impl MergeSession {
    #[must_use]
    pub fn alignment(&self) -> MergeAlignment {
        let local = Alignment::between(&self.local, &self.result);
        let incoming = Alignment::between(&self.result, &self.incoming);
        let mut rows = Vec::new();
        let mut left = 0;
        let mut right = 0;

        for result_line in 0..=self.result.lines().len() {
            let left_start = left;
            while local
                .rows()
                .get(left)
                .is_some_and(|row| row.right.is_none())
            {
                left += 1;
            }
            let right_start = right;
            while incoming
                .rows()
                .get(right)
                .is_some_and(|row| row.left.is_none())
            {
                right += 1;
            }

            for offset in 0..(left - left_start).max(right - right_start) {
                rows.push(MergeRow {
                    local: (offset < left - left_start)
                        .then(|| local.rows()[left_start + offset].left)
                        .flatten(),
                    result: None,
                    incoming: (offset < right - right_start)
                        .then(|| incoming.rows()[right_start + offset].right)
                        .flatten(),
                });
            }

            if result_line < self.result.lines().len() {
                rows.push(MergeRow {
                    local: local.rows().get(left).and_then(|row| row.left),
                    result: Some(result_line),
                    incoming: incoming.rows().get(right).and_then(|row| row.right),
                });
                left += 1;
                right += 1;
            }
        }

        let local_map = row_map(&rows, self.local.lines().len(), |row| row.local);
        let result_map = row_map(&rows, self.result.lines().len(), |row| row.result);
        let incoming_map = row_map(&rows, self.incoming.lines().len(), |row| row.incoming);
        let conflicts = self
            .conflicts
            .iter()
            .zip(&self.states)
            .map(|(conflict, state)| {
                conflict_span([
                    source_span(&self.local, &conflict.local, &local_map),
                    source_span(&self.result, &state.result, &result_map),
                    source_span(&self.incoming, &conflict.incoming, &incoming_map),
                ])
            })
            .collect();

        MergeAlignment { rows, conflicts }
    }
}

fn row_map(
    rows: &[MergeRow],
    lines: usize,
    source: impl Fn(&MergeRow) -> Option<usize>,
) -> Vec<usize> {
    let mut map = vec![0; lines];
    for (index, row) in rows.iter().enumerate() {
        if let Some(line) = source(row) {
            map[line] = index;
        }
    }

    map
}

fn source_span(document: &Document, bytes: &Range<usize>, map: &[usize]) -> Option<Range<usize>> {
    if bytes.is_empty() {
        return None;
    }

    let first = document.line_at_offset(bytes.start);
    let last = document.line_at_offset(bytes.end - 1);
    Some(map[first]..map[last] + 1)
}

fn conflict_span(spans: [Option<Range<usize>>; 3]) -> Range<usize> {
    let mut start = usize::MAX;
    let mut end = 0;
    for span in spans.into_iter().flatten() {
        start = start.min(span.start);
        end = end.max(span.end);
    }

    if start == usize::MAX {
        0..0
    } else {
        start..end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::{ConflictId, Take, byte_range};
    use yori_document::editing::TextSelection;

    fn doc(text: &str) -> Document {
        Document::from_bytes(text.as_bytes().to_vec()).unwrap()
    }

    fn reconstruct(document: &Document, lines: impl Iterator<Item = Option<usize>>) -> String {
        lines
            .flatten()
            .map(|line| document.copy_range(byte_range(document, line..line + 1)))
            .collect()
    }

    #[test]
    fn all_panes_reconstruct_exact_source_before_and_after_editing() {
        let mut merge = MergeSession::new(
            doc("head\r\nbase\r\ntail"),
            doc("local prefix\r\nhead\r\nlocal\r\ntail"),
            doc("head\r\nincoming\r\nmore\r\ntail\r\nend"),
        )
        .unwrap();

        for iteration in 0..3 {
            let alignment = merge.alignment();
            let rows = alignment.rows();
            assert_eq!(
                reconstruct(merge.local(), rows.iter().map(|row| row.local)),
                merge.local().text()
            );
            assert_eq!(
                reconstruct(merge.result(), rows.iter().map(|row| row.result)),
                merge.result().text()
            );
            assert_eq!(
                reconstruct(merge.incoming(), rows.iter().map(|row| row.incoming)),
                merge.incoming().text()
            );
            assert!(
                rows.iter().all(|row| row.local.is_some()
                    || row.result.is_some()
                    || row.incoming.is_some())
            );
            assert!(merge.conflicts().iter().all(|conflict| {
                alignment
                    .conflict_rows(conflict.id)
                    .is_some_and(|span| span.start < span.end && span.end <= rows.len())
            }));

            if iteration == 0 {
                merge
                    .replace(TextSelection::caret(0), 0..merge.result().text().len(), "")
                    .unwrap();
            } else if iteration == 1 {
                merge
                    .take(ConflictId(0), Take::Incoming, TextSelection::caret(0))
                    .unwrap();
            }
        }
    }

    #[test]
    fn empty_result_keeps_both_input_gaps_without_fabricating_bytes() {
        let merge = MergeSession::new(doc("old\n"), doc(""), doc("new\nsecond\n")).unwrap();
        let alignment = merge.alignment();

        assert_eq!(
            alignment.rows(),
            &[
                MergeRow {
                    local: None,
                    result: None,
                    incoming: Some(0)
                },
                MergeRow {
                    local: None,
                    result: None,
                    incoming: Some(1)
                },
            ]
        );
        assert_eq!(alignment.conflict_rows(ConflictId(0)), Some(0..2));
    }
}
