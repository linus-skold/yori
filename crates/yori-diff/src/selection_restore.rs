//! Translate an ordinary source selection into a line-scoped baseline restore.

use std::ops::Range;

use yori_document::{
    Document, InputError,
    editing::{EditHistory, EditOutcome, TextSelection},
};

use crate::{Alignment, DiffKind};

/// The exact display span and source ranges previewed before restoring selected
/// lines. Empty source ranges denote insertion/deletion, never phantom bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionRestore {
    pub rows: Range<usize>,
    pub baseline: Range<usize>,
    pub local: Range<usize>,
}

impl Alignment {
    /// Expand a nonempty source selection to touched lines, then trim unchanged
    /// edges. A selection ending at a line start does not include that line.
    /// Opposite-side gaps inside the selected span participate; adjacent gaps do
    /// not. A caret or selection containing only unchanged rows has no action.
    #[must_use]
    pub fn selection_restore(
        &self,
        baseline: &Document,
        local: &Document,
        selection: TextSelection,
        left_side: bool,
    ) -> Option<SelectionRestore> {
        let document = if left_side { baseline } else { local };
        let range = selection.range();
        if range.is_empty() || document.text().get(range.clone()).is_none() {
            return None;
        }

        let start = self.row_for_offset(document, range.start, left_side);
        let end = self.row_for_offset(document, range.end - 1, left_side) + 1;
        let selected = self.rows().get(start..end)?;
        let first = selected
            .iter()
            .position(|row| row.kind != DiffKind::Equal)?;
        let last = selected
            .iter()
            .rposition(|row| row.kind != DiffKind::Equal)?;
        let rows = start + first..start + last + 1;

        Some(SelectionRestore {
            baseline: self.source_range(baseline, rows.clone(), true),
            local: self.source_range(local, rows.clone(), false),
            rows,
        })
    }

    pub(crate) fn source_range(
        &self,
        document: &Document,
        rows: Range<usize>,
        left_side: bool,
    ) -> Range<usize> {
        let mut lines = self.rows()[rows.clone()]
            .iter()
            .filter_map(|row| if left_side { row.left } else { row.right });
        let Some(first) = lines.next() else {
            let offset = self.gap_offset(document, rows.start, left_side);
            return offset..offset;
        };

        let last = lines.next_back().unwrap_or(first);

        document.lines()[first].full.start..document.lines()[last].full.end
    }
}

/// Apply a plan from the current alignment as one undoable edit. Preserve exact
/// baseline bytes except where a missing EOF terminator would join a restored
/// line to retained local content: keep that boundary using the local newline.
pub fn restore_selection(
    history: &mut EditHistory,
    baseline: &Document,
    local: &mut Document,
    selection: TextSelection,
    plan: &SelectionRestore,
) -> Result<EditOutcome, InputError> {
    let source = baseline
        .text()
        .get(plan.baseline.clone())
        .ok_or(InputError::InvalidRange)?;
    if local.text().get(plan.local.clone()).is_none() {
        return Err(InputError::InvalidRange);
    }

    let mut replacement = String::new();
    if !source.is_empty()
        && plan.local.start > 0
        && !local.text()[..plan.local.start].ends_with('\n')
    {
        replacement.push_str(local.newline());
    }
    replacement.push_str(source);
    if !source.is_empty() && !source.ends_with('\n') && plan.local.end < local.text().len() {
        replacement.push_str(local.newline());
    }

    history.replace(local, selection, plan.local.clone(), &replacement)
}
