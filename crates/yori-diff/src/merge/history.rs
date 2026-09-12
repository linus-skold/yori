//! One chronological history for text deltas and explicit resolution decisions.

use yori_document::{
    Document,
    editing::{EditHistory, TextSelection},
};

use super::{ConflictState, MergeError, MergeUpdate};

#[derive(Clone)]
pub(super) struct State {
    conflicts: Vec<ConflictState>,
    selection: TextSelection,
}

impl State {
    pub fn new(conflicts: &[ConflictState], selection: TextSelection) -> Self {
        Self {
            conflicts: conflicts.to_vec(),
            selection,
        }
    }
}

struct Step {
    before: State,
    after: State,
    text_changed: bool,
}

struct Transaction {
    original: String,
    before: State,
}

#[derive(Default)]
pub(super) struct MergeHistory {
    undo: Vec<Step>,
    redo: Vec<Step>,
    transaction: Option<Transaction>,
}

impl MergeHistory {
    pub fn begin(
        &mut self,
        document: &Document,
        states: &[ConflictState],
        selection: TextSelection,
    ) {
        if self.transaction.is_none() {
            self.transaction = Some(Transaction {
                original: document.text().to_owned(),
                before: State::new(states, selection),
            });
        }
    }

    pub fn finish(
        &mut self,
        document: &Document,
        states: &mut Vec<ConflictState>,
        selection: TextSelection,
    ) {
        let Some(transaction) = self.transaction.take() else {
            return;
        };

        if transaction.original == document.text() {
            *states = transaction.before.conflicts;
            return;
        }

        self.record(transaction.before, State::new(states, selection), true);
    }

    pub fn record(&mut self, before: State, after: State, text_changed: bool) {
        if !text_changed && before.conflicts == after.conflicts {
            return;
        }

        self.redo.clear();
        if self.transaction.is_none() {
            self.undo.push(Step {
                before,
                after,
                text_changed,
            });
        }
    }

    pub fn undo(
        &mut self,
        result: &mut Document,
        edits: &mut EditHistory,
        states: &mut Vec<ConflictState>,
        selection: TextSelection,
    ) -> Result<Option<MergeUpdate>, MergeError> {
        let Some(step) = self.undo.last() else {
            return Ok(None);
        };
        let edit = if step.text_changed {
            Some(
                edits
                    .undo(result, selection)?
                    .ok_or(MergeError::HistoryInvariant)?,
            )
        } else {
            None
        };

        let step = self.undo.pop().expect("undo entry checked above");
        states.clone_from(&step.before.conflicts);
        let selection = step.before.selection;
        self.redo.push(step);

        Ok(Some(MergeUpdate { selection, edit }))
    }

    pub fn redo(
        &mut self,
        result: &mut Document,
        edits: &mut EditHistory,
        states: &mut Vec<ConflictState>,
        selection: TextSelection,
    ) -> Result<Option<MergeUpdate>, MergeError> {
        let Some(step) = self.redo.last() else {
            return Ok(None);
        };
        let edit = if step.text_changed {
            Some(
                edits
                    .redo(result, selection)?
                    .ok_or(MergeError::HistoryInvariant)?,
            )
        } else {
            None
        };

        let step = self.redo.pop().expect("redo entry checked above");
        states.clone_from(&step.after.conflicts);
        let selection = step.after.selection;
        self.undo.push(step);

        Ok(Some(MergeUpdate { selection, edit }))
    }
}
