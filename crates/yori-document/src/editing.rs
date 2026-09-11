//! Small source-based edit history and navigation for the editable prototype.
//! History stores replacement deltas, not full document snapshots or display rows.

use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

use crate::{Document, InputError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    pub anchor: usize,
    pub head: usize,
}

impl TextSelection {
    #[must_use]
    pub fn caret(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    #[must_use]
    pub fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

#[derive(Debug)]
pub struct EditOutcome {
    pub selection: TextSelection,
    pub replaced: Range<usize>,
    pub inserted_len: usize,
}

impl EditOutcome {
    /// Left affinity keeps a viewport anchor before newly inserted text at that anchor.
    #[must_use]
    pub fn map_anchor(&self, offset: usize) -> usize {
        if offset <= self.replaced.start {
            offset
        } else if offset >= self.replaced.end {
            self.replaced.start + self.inserted_len + offset - self.replaced.end
        } else {
            self.replaced.start
        }
    }
}

#[derive(Debug)]
struct Change {
    start: usize,
    removed: String,
    inserted: String,
    before: TextSelection,
    after: TextSelection,
}

#[derive(Debug)]
struct Composition {
    start: usize,
    removed: String,
    len: usize,
    before: TextSelection,
}

#[derive(Debug)]
struct Transaction {
    original: String,
    before: TextSelection,
}

#[derive(Debug, Default)]
pub struct EditHistory {
    undo: Vec<Change>,
    redo: Vec<Change>,
    composition: Option<Composition>,
    transaction: Option<Transaction>,
}

impl EditHistory {
    #[must_use]
    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.composition.as_ref().map(|c| c.start..c.start + c.len)
    }

    fn record(&mut self, change: Change) {
        if change.removed != change.inserted {
            if self.transaction.is_none() {
                self.undo.push(change);
            }
            self.redo.clear();
        }
    }

    /// Group an editing command and its subsequent typing into one undo step.
    /// Only the open transaction retains a snapshot; committed history remains deltas.
    pub fn begin_transaction(&mut self, document: &Document, selection: TextSelection) {
        if self.transaction.is_none() {
            self.finish_composition(document, selection);
            self.transaction = Some(Transaction {
                original: document.text().to_owned(),
                before: selection,
            });
        }
    }

    pub fn finish_transaction(&mut self, document: &Document, selection: TextSelection) {
        self.finish_composition(document, selection);
        let Some(transaction) = self.transaction.take() else {
            return;
        };

        let old = transaction.original;
        let new = document.text();
        let start = old
            .chars()
            .zip(new.chars())
            .take_while(|(left, right)| left == right)
            .map(|(ch, _)| ch.len_utf8())
            .sum::<usize>();
        let suffix = old[start..]
            .chars()
            .rev()
            .zip(new[start..].chars().rev())
            .take_while(|(left, right)| left == right)
            .map(|(ch, _)| ch.len_utf8())
            .sum::<usize>();

        self.record(Change {
            start,
            removed: old[start..old.len() - suffix].to_owned(),
            inserted: new[start..new.len() - suffix].to_owned(),
            before: transaction.before,
            after: selection,
        });
    }

    pub fn finish_composition(&mut self, document: &Document, selection: TextSelection) {
        if let Some(c) = self.composition.take() {
            self.record(Change {
                start: c.start,
                removed: c.removed,
                inserted: document.copy_range(c.start..c.start + c.len).to_owned(),
                before: c.before,
                after: selection,
            });
        }
    }

    pub fn replace(
        &mut self,
        document: &mut Document,
        selection: TextSelection,
        range: Range<usize>,
        text: &str,
    ) -> Result<EditOutcome, InputError> {
        // A committed IME replacement belongs to the entire composition transaction.
        if self.marked_range().as_ref() == Some(&range) {
            let outcome = self.replace_marked(document, selection, range, text, None)?;
            self.finish_composition(document, outcome.selection);
            return Ok(outcome);
        }

        let removed = document
            .text()
            .get(range.clone())
            .ok_or(InputError::InvalidRange)?
            .to_owned();

        // Validate the change before consuming a pending composition or redo history.
        let mut next = document.clone();
        next.replace(range.clone(), text)?;

        self.finish_composition(document, selection);
        *document = next;
        let after = TextSelection::caret(range.start + text.len());
        self.record(Change {
            start: range.start,
            removed,
            inserted: text.to_owned(),
            before: selection,
            after,
        });

        Ok(EditOutcome {
            selection: after,
            replaced: range,
            inserted_len: text.len(),
        })
    }

    pub fn replace_marked(
        &mut self,
        document: &mut Document,
        selection: TextSelection,
        range: Range<usize>,
        text: &str,
        selected_within: Option<Range<usize>>,
    ) -> Result<EditOutcome, InputError> {
        let selected = selected_within.unwrap_or(text.len()..text.len());
        if text.get(selected.clone()).is_none() {
            return Err(InputError::InvalidRange);
        }

        let removed = document
            .text()
            .get(range.clone())
            .ok_or(InputError::InvalidRange)?
            .to_owned();
        let mut next = document.clone();
        next.replace(range.clone(), text)?;

        if self.marked_range().as_ref() != Some(&range) {
            self.finish_composition(document, selection);
            self.composition = Some(Composition {
                start: range.start,
                removed,
                len: text.len(),
                before: selection,
            });
        }

        *document = next;
        self.composition
            .as_mut()
            .expect("composition established")
            .len = text.len();

        Ok(EditOutcome {
            selection: TextSelection {
                anchor: range.start + selected.start,
                head: range.start + selected.end,
            },
            replaced: range,
            inserted_len: text.len(),
        })
    }

    pub fn undo(
        &mut self,
        document: &mut Document,
        selection: TextSelection,
    ) -> Result<Option<EditOutcome>, InputError> {
        self.finish_transaction(document, selection);
        let Some(change) = self.undo.last() else {
            return Ok(None);
        };

        let range = change.start..change.start + change.inserted.len();
        document.replace(range.clone(), &change.removed)?;

        let outcome = EditOutcome {
            selection: change.before,
            replaced: range,
            inserted_len: change.removed.len(),
        };
        self.redo.push(self.undo.pop().unwrap());

        Ok(Some(outcome))
    }

    pub fn redo(
        &mut self,
        document: &mut Document,
        selection: TextSelection,
    ) -> Result<Option<EditOutcome>, InputError> {
        self.finish_transaction(document, selection);
        let Some(change) = self.redo.last() else {
            return Ok(None);
        };

        let range = change.start..change.start + change.removed.len();
        document.replace(range.clone(), &change.inserted)?;

        let outcome = EditOutcome {
            selection: change.after,
            replaced: range,
            inserted_len: change.inserted.len(),
        };
        self.undo.push(self.redo.pop().unwrap());

        Ok(Some(outcome))
    }
}

#[must_use]
pub fn previous_grapheme(text: &str, offset: usize) -> usize {
    text[..offset]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index)
}

#[must_use]
pub fn next_grapheme(text: &str, offset: usize) -> usize {
    text[offset..]
        .graphemes(true)
        .next()
        .map_or(text.len(), |cluster| offset + cluster.len())
}

/// Platform input uses UTF-16 units; source ranges remain UTF-8 bytes.
/// An interior surrogate offset is rounded to the start of its scalar value.
#[must_use]
pub fn from_utf16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units + ch.len_utf16() > offset {
            return byte;
        }

        units += ch.len_utf16();
    }

    text.len()
}

#[must_use]
pub fn to_utf16(text: &str, byte: usize) -> usize {
    text[..byte].encode_utf16().count()
}

#[derive(Clone, Copy)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Start,
    Finish,
}

pub fn navigate(
    document: &Document,
    selection: TextSelection,
    motion: Motion,
    extend: bool,
    preferred_column: &mut Option<usize>,
) -> TextSelection {
    let text = document.text();
    let range = selection.range();
    let head = selection.head;
    let line = document.line_at_offset(head);
    let content = document.line_content_range(line);

    let next = match motion {
        Motion::Left if !extend && !range.is_empty() => range.start,
        Motion::Right if !extend && !range.is_empty() => range.end,
        Motion::Left => previous_grapheme(text, head),
        Motion::Right => next_grapheme(text, head),
        Motion::Home => content.start,
        Motion::End => content.end,
        Motion::Start => 0,
        Motion::Finish => text.len(),
        Motion::Up | Motion::Down => {
            let column = *preferred_column.get_or_insert_with(|| {
                text[content.start..head.min(content.end)]
                    .graphemes(true)
                    .count()
            });

            let target = if matches!(motion, Motion::Up) {
                line.saturating_sub(1)
            } else {
                (line + 1).min(document.line_at_offset(text.len()))
            };
            let target = document.line_content_range(target);
            let offset = text[target.clone()]
                .grapheme_indices(true)
                .nth(column)
                .map_or(target.len(), |(offset, _)| offset);
            target.start + offset
        }
    };

    if !matches!(motion, Motion::Up | Motion::Down) {
        *preferred_column = None;
    }

    TextSelection {
        anchor: if extend { selection.anchor } else { next },
        head: next,
    }
}
