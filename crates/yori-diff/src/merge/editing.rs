//! Result editing owns paired text/decision history and conflict remapping.

use std::ops::Range;

use yori_document::{
    Document, InputError,
    editing::{EditUpdate, SourceEdit, TextSelection},
};

use super::{MergeSession, State, remap};

enum Replacement {
    Committed,
    Marked(Option<Range<usize>>),
}

pub struct MergeResultEdit<'a> {
    session: &'a mut MergeSession,
    selection: TextSelection,
    grouped: bool,
}

impl MergeSession {
    pub fn editing(&mut self, selection: TextSelection, grouped: bool) -> MergeResultEdit<'_> {
        MergeResultEdit {
            session: self,
            selection,
            grouped,
        }
    }
}

impl MergeResultEdit<'_> {
    fn replace_inner(
        &mut self,
        range: Range<usize>,
        text: &str,
        replacement: Replacement,
    ) -> Result<EditUpdate, InputError> {
        if let Replacement::Marked(Some(selected)) = &replacement
            && text.get(selected.clone()).is_none()
        {
            return Err(InputError::InvalidRange);
        }

        let session = &mut *self.session;
        let old = session
            .result
            .text()
            .get(range.clone())
            .ok_or(InputError::InvalidRange)?;
        let changed = old != text;

        // Validate before starting either history. In particular a rejected marked
        // selection must not consume an earlier composition or a redo branch.
        session.result.clone().replace(range.clone(), text)?;

        if self.grouped || matches!(replacement, Replacement::Marked(_)) {
            session.begin_transaction(self.selection);
        }
        let before = State::new(&session.states, self.selection);
        let owner = session.edit_owner(&range);
        let edit = if let Replacement::Marked(selected) = replacement {
            session.edits.replace_marked(
                &mut session.result,
                self.selection,
                range,
                text,
                selected,
            )?
        } else {
            session
                .edits
                .replace(&mut session.result, self.selection, range, text)?
        };

        if changed {
            remap(&mut session.states, &edit, owner);
        }
        self.selection = edit.selection;
        session
            .history
            .record(before, State::new(&session.states, self.selection), changed);

        Ok(EditUpdate {
            selection: self.selection,
            edit: changed.then_some(edit),
        })
    }
}

impl SourceEdit for MergeResultEdit<'_> {
    fn document(&self) -> &Document {
        self.session.result()
    }

    fn marked_range(&self) -> Option<Range<usize>> {
        self.session.marked_range()
    }

    fn replace(&mut self, range: Range<usize>, text: &str) -> Result<EditUpdate, InputError> {
        self.replace_inner(range, text, Replacement::Committed)
    }

    fn replace_marked(
        &mut self,
        range: Range<usize>,
        text: &str,
        selected_within: Option<Range<usize>>,
    ) -> Result<EditUpdate, InputError> {
        self.replace_inner(range, text, Replacement::Marked(selected_within))
    }
}
