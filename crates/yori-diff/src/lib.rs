//! Headless line comparison, alignment, and undoable baseline restoration.

use similar::{Algorithm, DiffTag, TextDiff};
use std::ops::Range;
use yori_document::{
    Document, InputError,
    editing::{EditHistory, EditOutcome, TextSelection},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Equal,
    Removed,
    Added,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignmentRow {
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub kind: DiffKind,
}

/// One contiguous changed run, without unchanged context. Byte ranges include
/// original line terminators; an absent side has an empty insertion range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeBlock {
    pub rows: Range<usize>,
    pub left: Range<usize>,
    pub right: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Alignment {
    rows: Vec<AlignmentRow>,
    blocks: Vec<ChangeBlock>,
}

impl Alignment {
    #[must_use]
    pub fn between(left: &Document, right: &Document) -> Self {
        let left_lines: Vec<_> = left
            .lines()
            .iter()
            .map(|line| left.copy_range(line.full.clone()))
            .collect();
        let right_lines: Vec<_> = right
            .lines()
            .iter()
            .map(|line| right.copy_range(line.full.clone()))
            .collect();
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Myers)
            .diff_slices(&left_lines, &right_lines);
        let mut rows = Vec::new();
        let mut blocks: Vec<ChangeBlock> = Vec::new();
        let byte_boundary = |doc: &Document, line: usize| {
            doc.lines()
                .get(line)
                .map_or(doc.text().len(), |line| line.full.start)
        };
        for op in diff.ops() {
            let old = op.old_range();
            let new = op.new_range();
            if op.tag() != DiffTag::Equal {
                let end_row = rows.len() + old.len().max(new.len());
                let left_bytes = byte_boundary(left, old.start)..byte_boundary(left, old.end);
                let right_bytes = byte_boundary(right, new.start)..byte_boundary(right, new.end);
                if let Some(block) = blocks
                    .last_mut()
                    .filter(|block| block.rows.end == rows.len())
                {
                    block.rows.end = end_row;
                    block.left.end = left_bytes.end;
                    block.right.end = right_bytes.end;
                } else {
                    blocks.push(ChangeBlock {
                        rows: rows.len()..end_row,
                        left: left_bytes,
                        right: right_bytes,
                    });
                }
            }
            match op.tag() {
                DiffTag::Equal => {
                    rows.extend(old.zip(new).map(|(left, right)| AlignmentRow {
                        left: Some(left),
                        right: Some(right),
                        kind: DiffKind::Equal,
                    }));
                }
                DiffTag::Delete => rows.extend(old.map(|left| AlignmentRow {
                    left: Some(left),
                    right: None,
                    kind: DiffKind::Removed,
                })),
                DiffTag::Insert => rows.extend(new.map(|right| AlignmentRow {
                    left: None,
                    right: Some(right),
                    kind: DiffKind::Added,
                })),
                DiffTag::Replace => {
                    let count = old.len().max(new.len());
                    for offset in 0..count {
                        let left_line = (offset < old.len()).then_some(old.start + offset);
                        let right_line = (offset < new.len()).then_some(new.start + offset);
                        rows.push(AlignmentRow {
                            left: left_line,
                            right: right_line,
                            kind: match (left_line, right_line) {
                                (Some(_), Some(_)) => DiffKind::Modified,
                                (Some(_), None) => DiffKind::Removed,
                                (None, Some(_)) => DiffKind::Added,
                                (None, None) => unreachable!(),
                            },
                        });
                    }
                }
            }
        }
        Self { rows, blocks }
    }

    #[must_use]
    pub fn blocks(&self) -> &[ChangeBlock] {
        &self.blocks
    }

    #[must_use]
    pub fn rows(&self) -> &[AlignmentRow] {
        &self.rows
    }

    /// Locate a source cursor without ever putting it inside alignment-only content.
    #[must_use]
    pub fn row_for_offset(&self, document: &Document, offset: usize, left_side: bool) -> usize {
        let source_line = document.line_at_offset(offset);
        let side_line = |row: &AlignmentRow| if left_side { row.left } else { row.right };
        if source_line < document.lines().len() {
            self.rows
                .iter()
                .position(|row| side_line(row) == Some(source_line))
                .unwrap_or(0)
        } else {
            self.rows
                .iter()
                .rposition(|row| side_line(row).is_some())
                .map_or(0, |row| row + 1)
        }
    }

    /// Maps a presentation row without source content to the one source boundary
    /// shared by its neighboring real lines. Leading/trailing gaps map to 0/EOF.
    #[must_use]
    pub fn gap_offset(&self, document: &Document, row: usize, left_side: bool) -> usize {
        let line_at = |candidate: &AlignmentRow| {
            if left_side {
                candidate.left
            } else {
                candidate.right
            }
        };
        if let Some(line) = self.rows.get(row).and_then(line_at) {
            return document.lines()[line].content.start;
        }
        for candidate in self.rows[..row.min(self.rows.len())].iter().rev() {
            if let Some(line) = line_at(candidate) {
                return document.lines()[line].full.end;
            }
        }
        for candidate in self.rows.get(row.saturating_add(1)..).unwrap_or_default() {
            if let Some(line) = line_at(candidate) {
                return document.lines()[line].full.start;
            }
        }
        0
    }
}

/// Restore exact baseline bytes as one ordinary undoable replacement.
/// The caller supplies a block from the current document alignment.
pub fn restore_block(
    history: &mut EditHistory,
    baseline: &Document,
    document: &mut Document,
    selection: TextSelection,
    block: &ChangeBlock,
) -> Result<EditOutcome, InputError> {
    let text = baseline
        .text()
        .get(block.left.clone())
        .ok_or(InputError::InvalidRange)?;
    history.replace(document, selection, block.right.clone(), text)
}
