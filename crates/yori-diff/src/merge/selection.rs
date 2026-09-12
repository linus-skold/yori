//! Apply line-scoped source takes through the merge session's unified history.

#[cfg(test)]
mod tests;

use std::ops::Range;
use yori_document::editing::{EditOutcome, TextSelection};

use super::{MergeError, MergeSession, MergeUpdate, history::State};
use crate::{SelectionRestore, restore_selection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeInput {
    Local,
    Incoming,
}

impl MergeSession {
    /// Apply a plan from the current source/result projection without resolving
    /// or reopening any conflict. The caller must revalidate a retained preview
    /// against its current selection and projection before invoking this method.
    pub fn take_lines(
        &mut self,
        input: MergeInput,
        selection: TextSelection,
        plan: &SelectionRestore,
    ) -> Result<MergeUpdate, MergeError> {
        let source = match input {
            MergeInput::Local => self.local.clone(),
            MergeInput::Incoming => self.incoming.clone(),
        };

        self.finish_transaction(selection);
        let before = State::new(&self.states, selection);
        let original = self.result.text().to_owned();
        let prefix = if !plan.baseline.is_empty()
            && plan.local.start > 0
            && original
                .get(..plan.local.start)
                .is_some_and(|text| !text.ends_with('\n'))
        {
            self.result.newline().len()
        } else {
            0
        };

        let edit = restore_selection(&mut self.edits, &source, &mut self.result, selection, plan)?;
        let changed = original != self.result.text();
        if changed {
            // Unlike free typing, a take has known input provenance. Preserve
            // each conflict's own portion instead of assigning a multi-conflict
            // replacement wholesale to the first touched conflict.
            for (conflict, state) in self.conflicts.iter().zip(&mut self.states) {
                let input_range = match input {
                    MergeInput::Local => &conflict.local,
                    MergeInput::Incoming => &conflict.incoming,
                };
                state.result = remap_selected(&state.result, input_range, plan, &edit, prefix);
            }
        }

        self.history
            .record(before, State::new(&self.states, edit.selection), changed);

        Ok(MergeUpdate {
            selection: edit.selection,
            edit: changed.then_some(edit),
        })
    }
}

fn remap_selected(
    old: &Range<usize>,
    input: &Range<usize>,
    plan: &SelectionRestore,
    edit: &EditOutcome,
    prefix: usize,
) -> Range<usize> {
    let start = edit.replaced.start;
    let end = edit.replaced.end;
    let new_end = start + edit.inserted_len;
    let after = |offset| new_end + (offset - end);
    let mut portions = Vec::with_capacity(3);

    if old.start < start {
        portions.push(old.start..old.end.min(start));
    }
    if old.end > end {
        portions.push(after(old.start.max(end))..after(old.end));
    }

    let first = input.start.max(plan.baseline.start);
    let last = input.end.min(plan.baseline.end);
    if first < last {
        let mapped_start = if first == plan.baseline.start {
            start
        } else {
            start + prefix + first - plan.baseline.start
        };
        let mapped_end = if last == plan.baseline.end {
            // Any boundary newline appended to this final source fragment must
            // belong to the same conflict as that fragment.
            new_end
        } else {
            start + prefix + last - plan.baseline.start
        };
        portions.push(mapped_start..mapped_end);
    }

    if let (Some(first), Some(last)) = (
        portions.iter().map(|range| range.start).min(),
        portions.iter().map(|range| range.end).max(),
    ) {
        return first..last;
    }

    let anchor = if old.start >= start && old.end <= end {
        start + prefix + input.start.clamp(plan.baseline.start, plan.baseline.end)
            - plan.baseline.start
    } else if old.start >= end {
        after(old.start)
    } else {
        old.start
    };

    anchor..anchor
}
