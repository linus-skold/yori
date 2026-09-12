//! Constrained source mutation shared by native input and modal commands.

use std::ops::Range;

use super::{EditHistory, EditOutcome, TextSelection};
use crate::{Document, InputError};

#[derive(Debug)]
pub struct EditUpdate {
    pub selection: TextSelection,
    /// Selection/composition and merge status can change without changing text.
    pub edit: Option<EditOutcome>,
}

/// Source coordinates are UTF-8 bytes, never display rows or platform UTF-16 units.
/// Every accepted replacement maintains its owner's history immediately; discarding
/// the returned presentation update cannot discard or corrupt the source edit.
pub trait SourceEdit {
    fn document(&self) -> &Document;
    fn marked_range(&self) -> Option<Range<usize>>;
    fn replace(&mut self, range: Range<usize>, text: &str) -> Result<EditUpdate, InputError>;
    fn replace_marked(
        &mut self,
        range: Range<usize>,
        text: &str,
        selected_within: Option<Range<usize>>,
    ) -> Result<EditUpdate, InputError>;
}

/// Ordinary document editing. Grouping starts only after replacement validation,
/// so rejected input cannot retire an existing composition or consume redo.
pub struct DocumentEdit<'a> {
    document: &'a mut Document,
    history: &'a mut EditHistory,
    selection: TextSelection,
    grouped: bool,
}

impl<'a> DocumentEdit<'a> {
    pub fn new(
        document: &'a mut Document,
        history: &'a mut EditHistory,
        selection: TextSelection,
        grouped: bool,
    ) -> Self {
        Self {
            document,
            history,
            selection,
            grouped,
        }
    }

    fn prepare(&mut self, range: Range<usize>, text: &str) -> Result<(), InputError> {
        if self.grouped {
            self.document.clone().replace(range, text)?;
            self.history
                .begin_transaction(self.document, self.selection);
        }

        Ok(())
    }

    fn update(&mut self, edit: EditOutcome) -> EditUpdate {
        self.selection = edit.selection;

        EditUpdate {
            selection: edit.selection,
            edit: Some(edit),
        }
    }
}

impl SourceEdit for DocumentEdit<'_> {
    fn document(&self) -> &Document {
        self.document
    }

    fn marked_range(&self) -> Option<Range<usize>> {
        self.history.marked_range()
    }

    fn replace(&mut self, range: Range<usize>, text: &str) -> Result<EditUpdate, InputError> {
        self.prepare(range.clone(), text)?;
        let edit = self
            .history
            .replace(self.document, self.selection, range, text)?;

        Ok(self.update(edit))
    }

    fn replace_marked(
        &mut self,
        range: Range<usize>,
        text: &str,
        selected_within: Option<Range<usize>>,
    ) -> Result<EditUpdate, InputError> {
        if let Some(selected) = &selected_within
            && text.get(selected.clone()).is_none()
        {
            return Err(InputError::InvalidRange);
        }

        self.prepare(range.clone(), text)?;
        let edit = self.history.replace_marked(
            self.document,
            self.selection,
            range,
            text,
            selected_within,
        )?;

        Ok(self.update(edit))
    }
}
