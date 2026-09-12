//! A bounded Vim-style command layer over yori's existing document and history.
//!
//! There is no second editable buffer. Native typing continues through the normal
//! edit path in Insert mode; this module handles commands and source selections.

mod keys;
mod motions;

use std::ops::Range;

use yori_document::{
    Document, InputError,
    editing::{self, EditHistory, EditOutcome, TextSelection},
};

use keys::{Command, Insert, Keys, Motion, Operator, Target};
use motions::{first_word, inner_word, line_range, motion_range, move_to, normal_cursor};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl Mode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::VisualLine => "VISUAL LINE",
        }
    }
}

/// The session's single unnamed register. Linewise shape is not encoded as fake source bytes.
#[derive(Clone, Default)]
pub struct Register {
    text: String,
    linewise: bool,
}

struct OperationRange {
    bytes: Range<usize>,
    linewise: bool,
}

pub struct Outcome {
    pub selection: TextSelection,
    pub edit: Option<EditOutcome>,
    /// False only for native Insert-mode input.
    pub consumed: bool,
}

#[derive(Default)]
pub struct Vim {
    mode: Mode,
    keys: Keys,
    // Visual endpoints are inclusive source cursors, unlike the UI's half-open selection.
    visual: Option<(usize, usize)>,
    column: Option<usize>,
}

impl Vim {
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    #[must_use]
    pub fn cursor(&self, selection: TextSelection) -> usize {
        self.visual.map_or(selection.head, |(_, head)| head)
    }

    /// Mouse placement ends a typing transaction without taking an Insert user out of Insert.
    pub fn reposition(
        &mut self,
        document: &Document,
        history: &mut EditHistory,
        selection: TextSelection,
    ) {
        let inserting = self.mode == Mode::Insert;
        self.cancel(document, history, selection);

        if inserting {
            self.mode = Mode::Insert;
        }
    }

    /// Import a real mouse/keyboard selection without introducing a second selection model.
    pub fn select(&mut self, document: &Document, selection: TextSelection) {
        self.keys.clear();
        self.column = None;
        if self.mode == Mode::Insert {
            return;
        }

        let range = selection.range();
        if range.is_empty() {
            self.visual = None;
            self.mode = Mode::Normal;
            return;
        }

        let last = editing::previous_grapheme(document.text(), range.end);
        self.visual = Some(if selection.head < selection.anchor {
            (last, range.start)
        } else {
            (range.start, last)
        });
        self.mode = Mode::Visual;
    }

    /// End a modal interaction before focus loss or disabling Vim.
    /// This leaves the caller's selection intact and commits, rather than discards, typing.
    pub fn cancel(
        &mut self,
        document: &Document,
        history: &mut EditHistory,
        selection: TextSelection,
    ) {
        history.finish_transaction(document, selection);
        *self = Self::default();
    }

    /// Route history keys through a host's richer history (for example merge
    /// text plus resolution status) without changing command-prefix handling.
    pub fn external_history_key(&mut self, key: &str) -> Option<bool> {
        if self.mode == Mode::Insert || !matches!(key, "u" | "ctrl-r") {
            return None;
        }

        match self.keys.feed(key, self.visual.is_some()) {
            Some(Command::Undo) => Some(false),
            Some(Command::Redo) => Some(true),
            _ => None,
        }
    }

    pub fn handle(
        &mut self,
        key: &str,
        document: &mut Document,
        history: &mut EditHistory,
        selection: TextSelection,
        register: &mut Register,
        writable: bool,
    ) -> Result<Outcome, InputError> {
        if key == "escape" {
            let head = if self.mode == Mode::Insert {
                let content = document.line_content_range(document.line_at_offset(selection.head));
                editing::previous_grapheme(document.text(), selection.head).max(content.start)
            } else {
                self.visual.map_or(selection.head, |(_, head)| head)
            };

            let selection = TextSelection::caret(normal_cursor(document, head));
            self.cancel(document, history, selection);

            return Ok(Self::outcome(selection, None));
        }
        if self.mode == Mode::Insert {
            return Ok(Outcome {
                selection,
                edit: None,
                consumed: false,
            });
        }

        let visual = self.visual.is_some();
        let Some(command) = self.keys.feed(key, visual) else {
            return Ok(Self::outcome(selection, None));
        };

        let cursor = normal_cursor(
            document,
            self.visual.map_or(selection.head, |(_, head)| head),
        );
        let mutable = !matches!(
            command,
            Command::Move(..) | Command::Visual(_) | Command::Operate(Operator::Yank, ..)
        );
        if mutable && !writable {
            return Ok(Self::outcome(selection, None));
        }

        self.execute(command, document, history, selection, cursor, register)
    }

    fn execute(
        &mut self,
        command: Command,
        document: &mut Document,
        history: &mut EditHistory,
        selection: TextSelection,
        cursor: usize,
        register: &mut Register,
    ) -> Result<Outcome, InputError> {
        match command {
            Command::Move(motion, count) => {
                let head = normal_cursor(
                    document,
                    move_to(document, cursor, motion, count, &mut self.column),
                );
                let selection = if let Some((anchor, _)) = self.visual {
                    self.visual = Some((anchor, head));
                    self.visual_selection(document, anchor, head)
                } else {
                    TextSelection::caret(head)
                };

                Ok(Self::outcome(selection, None))
            }
            Command::Visual(linewise) => {
                let mode = if linewise {
                    Mode::VisualLine
                } else {
                    Mode::Visual
                };
                if self.mode == mode {
                    self.mode = Mode::Normal;
                    self.visual = None;

                    return Ok(Self::outcome(TextSelection::caret(cursor), None));
                }

                self.mode = mode;
                let anchor = self.visual.map_or(cursor, |(anchor, _)| anchor);
                self.visual = Some((anchor, cursor));

                Ok(Self::outcome(
                    self.visual_selection(document, anchor, cursor),
                    None,
                ))
            }
            Command::Insert(insert) => self.insert(insert, document, history, selection, cursor),
            Command::Operate(operator, target, count) => {
                let (range, linewise) =
                    self.target(document, selection, cursor, operator, target, count);
                let target = OperationRange {
                    bytes: range,
                    linewise,
                };

                self.operate(operator, document, history, selection, target, register)
            }
            Command::DeleteChar(count) => {
                let (range, linewise) = if self.visual.is_some() {
                    (selection.range(), self.mode == Mode::VisualLine)
                } else {
                    (
                        motion_range(document, cursor, Motion::Right, count, false).0,
                        false,
                    )
                };

                self.operate(
                    Operator::Delete,
                    document,
                    history,
                    selection,
                    OperationRange {
                        bytes: range,
                        linewise,
                    },
                    register,
                )
            }
            Command::Paste(after, count) => {
                self.paste(document, history, selection, register, after, count)
            }
            Command::Undo | Command::Redo => {
                self.cancel(document, history, selection);

                let edit = if command == Command::Undo {
                    history.undo(document, selection)?
                } else {
                    history.redo(document, selection)?
                };
                let next = edit.as_ref().map_or(selection, |edit| edit.selection);

                Ok(Self::outcome(
                    TextSelection::caret(normal_cursor(document, next.head)),
                    edit,
                ))
            }
        }
    }

    fn outcome(selection: TextSelection, edit: Option<EditOutcome>) -> Outcome {
        Outcome {
            selection,
            edit,
            consumed: true,
        }
    }

    fn visual_selection(&self, document: &Document, anchor: usize, head: usize) -> TextSelection {
        if self.mode == Mode::VisualLine {
            let range = line_range(document, anchor, head);
            return if head < anchor {
                TextSelection {
                    anchor: range.end,
                    head: range.start,
                }
            } else {
                TextSelection {
                    anchor: range.start,
                    head: range.end,
                }
            };
        }

        if head < anchor {
            TextSelection {
                anchor: editing::next_grapheme(document.text(), anchor),
                head,
            }
        } else {
            TextSelection {
                anchor,
                head: editing::next_grapheme(document.text(), head),
            }
        }
    }

    fn target(
        &self,
        document: &Document,
        selection: TextSelection,
        cursor: usize,
        operator: Operator,
        target: Target,
        count: usize,
    ) -> (Range<usize>, bool) {
        match target {
            Target::Selection => (selection.range(), self.mode == Mode::VisualLine),
            Target::InnerWord => (inner_word(document, cursor, count), false),
            Target::Motion(motion) => motion_range(
                document,
                cursor,
                motion,
                count,
                operator == Operator::Change,
            ),
            Target::Lines => {
                let end = document
                    .line_content_range(document.line_at_offset(cursor) + count - 1)
                    .start;
                (line_range(document, cursor, end), true)
            }
        }
    }

    fn operate(
        &mut self,
        operator: Operator,
        document: &mut Document,
        history: &mut EditHistory,
        selection: TextSelection,
        target: OperationRange,
        register: &mut Register,
    ) -> Result<Outcome, InputError> {
        let OperationRange {
            bytes: mut range,
            linewise,
        } = target;
        if range.is_empty() && operator != Operator::Change {
            return Ok(Self::outcome(selection, None));
        }

        let copied = Register {
            text: document.copy_range(range.clone()).to_owned(),
            linewise,
        };

        self.mode = Mode::Normal;
        self.visual = None;
        self.column = None;

        if operator == Operator::Yank {
            *register = copied;
            return Ok(Self::outcome(
                TextSelection::caret(normal_cursor(document, range.start)),
                None,
            ));
        }

        let replacement = if operator == Operator::Change
            && linewise
            && range.end > range.start
            && document.text()[range.clone()].ends_with('\n')
        {
            document.newline()
        } else {
            ""
        };

        // Removing an unterminated final line also removes its preceding separator.
        // The register still contains only the selected line(s).
        if operator == Operator::Delete
            && linewise
            && range.end == document.text().len()
            && !document.text().ends_with('\n')
            && range.start > 0
        {
            range.start = editing::previous_grapheme(document.text(), range.start);
        }

        if operator == Operator::Change {
            history.begin_transaction(document, selection);
        }
        let edit = history.replace(document, selection, range.clone(), replacement)?;

        *register = copied;
        let head = if operator == Operator::Change {
            self.mode = Mode::Insert;
            range.start
        } else {
            normal_cursor(document, range.start)
        };

        Ok(Self::outcome(TextSelection::caret(head), Some(edit)))
    }

    fn insert(
        &mut self,
        insert: Insert,
        document: &mut Document,
        history: &mut EditHistory,
        selection: TextSelection,
        cursor: usize,
    ) -> Result<Outcome, InputError> {
        let content = document.line_content_range(document.line_at_offset(cursor));
        let offset = match insert {
            Insert::Here => cursor,
            Insert::After => editing::next_grapheme(document.text(), cursor).min(content.end),
            Insert::First => first_word(document, cursor),
            Insert::End | Insert::Below => content.end,
            Insert::Above => content.start,
        };

        history.begin_transaction(document, selection);
        self.mode = Mode::Insert;
        self.visual = None;
        self.column = None;

        if matches!(insert, Insert::Above | Insert::Below) {
            let edit = history.replace(document, selection, offset..offset, document.newline())?;
            let head = if insert == Insert::Above {
                offset
            } else {
                edit.selection.head
            };

            return Ok(Self::outcome(TextSelection::caret(head), Some(edit)));
        }

        Ok(Self::outcome(TextSelection::caret(offset), None))
    }

    fn paste(
        &mut self,
        document: &mut Document,
        history: &mut EditHistory,
        selection: TextSelection,
        register: &mut Register,
        after: bool,
        count: usize,
    ) -> Result<Outcome, InputError> {
        if register.text.is_empty() {
            return Ok(Self::outcome(selection, None));
        }

        let cursor = normal_cursor(
            document,
            self.visual.map_or(selection.head, |(_, head)| head),
        );
        let mut text = register.text.clone();
        let linewise = register.linewise;
        if linewise && !text.ends_with('\n') {
            text.push_str(document.newline());
        }
        let mut text = text.repeat(count);

        let content = document.line_content_range(document.line_at_offset(cursor));
        let range = if self.visual.is_some() {
            selection.range()
        } else {
            let offset = if linewise {
                if after {
                    line_range(document, cursor, cursor).end
                } else {
                    content.start
                }
            } else if after {
                editing::next_grapheme(document.text(), cursor).min(content.end)
            } else {
                cursor
            };
            offset..offset
        };

        let mut head = range.start;
        if linewise
            && range.start == document.text().len()
            && !document.text().is_empty()
            && !document.text().ends_with('\n')
        {
            let ending_len = if text.ends_with("\r\n") { 2 } else { 1 };
            text.truncate(text.len() - ending_len);
            text.insert_str(0, document.newline());
            head += document.newline().len();
        }
        if self.visual.is_some() {
            *register = Register {
                text: document.copy_range(range.clone()).to_owned(),
                linewise: self.mode == Mode::VisualLine,
            };
        }

        self.mode = Mode::Normal;
        self.visual = None;
        self.column = None;

        let edit = history.replace(document, selection, range, &text)?;
        if !linewise {
            head = editing::previous_grapheme(document.text(), edit.selection.head);
        }

        Ok(Self::outcome(
            TextSelection::caret(normal_cursor(document, head)),
            Some(edit),
        ))
    }
}

#[cfg(test)]
mod tests;
