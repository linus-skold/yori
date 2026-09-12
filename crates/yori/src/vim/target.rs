//! Headless routing for modal and composition lifetime. Commands only receive
//! `SourceEdit`; the host, not the parser or native caller, coordinates histories.

use std::ops::Range;

use yori_diff::merge::{MergeError, MergeSession};
use yori_document::{
    Document, InputError,
    editing::{DocumentEdit, EditHistory, EditUpdate, SourceEdit, TextSelection},
};

pub enum EditTarget<'a> {
    Document(&'a mut Document, &'a mut EditHistory),
    Merge(&'a mut MergeSession),
    ReadOnly(&'a Document),
}

impl EditTarget<'_> {
    pub(super) fn document(&self) -> &Document {
        match self {
            Self::Document(document, _) => document,
            Self::ReadOnly(document) => document,
            Self::Merge(session) => session.result(),
        }
    }

    pub(super) fn writable(&self) -> bool {
        !matches!(self, Self::ReadOnly(_))
    }

    pub(super) fn begin(&mut self, selection: TextSelection) {
        match self {
            Self::Document(document, history) => history.begin_transaction(document, selection),
            Self::Merge(session) => session.begin_transaction(selection),
            Self::ReadOnly(_) => {}
        }
    }

    pub(super) fn finish(&mut self, selection: TextSelection) -> bool {
        match self {
            Self::Document(document, history) => {
                history.finish_transaction(document, selection);
                false
            }
            Self::Merge(session) => session.finish_transaction(selection),
            Self::ReadOnly(_) => false,
        }
    }

    /// Retire composition using the owner's grouping policy. Returns whether
    /// conflict ranges were restored, without inventing a text edit.
    pub fn unmark(&mut self, selection: TextSelection) -> bool {
        match self {
            // Preserve the native hosts' established distinction: merge unmark
            // closes the outer group; ordinary unmark only retires composition.
            Self::Document(document, history) => {
                history.finish_composition(document, selection);
                false
            }
            Self::Merge(session) => session.finish_transaction(selection),
            Self::ReadOnly(_) => false,
        }
    }

    pub(super) fn edit<T>(
        &mut self,
        selection: TextSelection,
        grouped: bool,
        command: impl FnOnce(&mut dyn SourceEdit) -> Result<T, InputError>,
    ) -> Result<T, InputError> {
        match self {
            Self::Document(document, history) => command(&mut DocumentEdit::new(
                document, history, selection, grouped,
            )),
            Self::Merge(session) => command(&mut session.editing(selection, grouped)),
            Self::ReadOnly(document) => command(&mut ReadOnly(document)),
        }
    }

    pub fn replace(
        &mut self,
        selection: TextSelection,
        range: Range<usize>,
        text: &str,
        inserting: bool,
    ) -> Result<EditUpdate, InputError> {
        self.edit(selection, inserting, |source| source.replace(range, text))
    }

    pub fn replace_marked(
        &mut self,
        selection: TextSelection,
        range: Range<usize>,
        text: &str,
        selected: Option<Range<usize>>,
        inserting: bool,
    ) -> Result<EditUpdate, InputError> {
        self.edit(selection, inserting, |source| {
            source.replace_marked(range, text, selected)
        })
    }

    pub(super) fn travel(
        &mut self,
        selection: TextSelection,
        redo: bool,
    ) -> Result<Option<EditUpdate>, MergeError> {
        match self {
            Self::Document(document, history) => {
                let edit = if redo {
                    history.redo(document, selection)?
                } else {
                    history.undo(document, selection)?
                };

                Ok(edit.map(|edit| EditUpdate {
                    selection: edit.selection,
                    edit: Some(edit),
                }))
            }
            Self::Merge(session) => {
                if redo {
                    session.redo(selection)
                } else {
                    session.undo(selection)
                }
            }
            Self::ReadOnly(_) => Ok(None),
        }
    }
}

struct ReadOnly<'a>(&'a Document);

impl SourceEdit for ReadOnly<'_> {
    fn document(&self) -> &Document {
        self.0
    }

    fn marked_range(&self) -> Option<Range<usize>> {
        None
    }

    fn replace(&mut self, _: Range<usize>, _: &str) -> Result<EditUpdate, InputError> {
        Err(InputError::InvalidRange)
    }

    fn replace_marked(
        &mut self,
        _: Range<usize>,
        _: &str,
        _: Option<Range<usize>>,
    ) -> Result<EditUpdate, InputError> {
        Err(InputError::InvalidRange)
    }
}
