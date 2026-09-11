//! Native GPUI routing for the bounded, headless Vim command layer.

use std::{cell::RefCell, rc::Rc};

use gpui_kit::{App, Context, Global, KeyDownEvent, Window};
use yori::vim::{Mode, Register};
use yori_document::editing::TextSelection;

use super::{AlignedEditor, Selection, Side};

#[derive(Default)]
pub(super) struct VimPreferences {
    pub enabled: bool,
    register: Rc<RefCell<Register>>,
}

impl Global for VimPreferences {}

#[cfg(test)]
mod tests;

pub(super) fn init(cx: &mut App) {
    cx.set_global(VimPreferences::default());
}

impl AlignedEditor {
    pub(super) fn vim_enabled(cx: &App) -> bool {
        cx.global::<VimPreferences>().enabled
    }

    pub(super) fn accepts_text(&self, cx: &App) -> bool {
        !Self::vim_enabled(cx) || self.vim.mode() == Mode::Insert
    }

    pub(super) fn cancel_vim(&mut self) {
        // History always belongs to the editable pane, even when focus moved left.
        let selection = self.right_selection().unwrap_or(TextSelection::caret(0));
        self.vim
            .cancel(&self.right.document, &mut self.history, selection);
    }

    pub(super) fn reposition_vim(&mut self) {
        let selection = self.right_selection().unwrap_or(TextSelection::caret(0));
        self.vim
            .reposition(&self.right.document, &mut self.history, selection);
    }

    pub(super) fn sync_vim_selection(&mut self, cx: &App) {
        if !Self::vim_enabled(cx) {
            return;
        }
        let Some(selection) = &self.selection else {
            return;
        };

        let document = match selection.side {
            Side::Left => &self.left.document,
            Side::Right => &self.right.document,
        };
        let selection = TextSelection {
            anchor: selection.anchor,
            head: selection.head,
        };
        self.vim.select(document, selection);
    }

    pub(super) fn vim_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !Self::vim_enabled(cx) {
            return;
        }

        let stroke = &event.keystroke;
        let redo = stroke.modifiers.control
            && !stroke.modifiers.alt
            && !stroke.modifiers.platform
            && stroke.key == "r";
        // Keep application shortcuts (tabs, change navigation, clipboard) outside modal parsing.
        if !redo && (stroke.modifiers.control || stroke.modifiers.platform || stroke.modifiers.alt)
        {
            if self.vim.mode() != Mode::Insert {
                self.cancel_vim();
            }
            return;
        }

        let key = if redo {
            "ctrl-r"
        } else {
            stroke.key_char.as_deref().unwrap_or(&stroke.key)
        };
        if self.vim.mode() == Mode::Insert && key != "escape" {
            return;
        }

        let selection = self.selection.clone().unwrap_or(Selection {
            side: Side::Right,
            anchor: 0,
            head: 0,
        });
        let old = TextSelection {
            anchor: selection.anchor,
            head: selection.head,
        };
        let anchor = self.view_anchor();
        let register = Rc::clone(&cx.global::<VimPreferences>().register);
        let document = match selection.side {
            Side::Left => &mut self.left.document,
            Side::Right => &mut self.right.document,
        };

        let outcome = self.vim.handle(
            key,
            document,
            &mut self.history,
            old,
            &mut register.borrow_mut(),
            selection.side == Side::Right,
        );

        match outcome {
            Ok(outcome) => {
                if let Some(edit) = &outcome.edit {
                    self.finish_edit(&anchor, edit, window, cx);
                }

                self.selection = Some(Selection {
                    side: selection.side,
                    anchor: outcome.selection.anchor,
                    head: outcome.selection.head,
                });
                self.preferred_column = None;

                self.locate_caret_change();
                self.reveal_cursor(window, cx);
            }
            Err(error) => {
                eprintln!("Vim command rejected: {error}");
                self.cancel_vim();
                window.play_system_bell();
            }
        }

        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn toggle_vim(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_vim();
        cx.global_mut::<VimPreferences>().enabled = enabled;
        if enabled && self.selection.is_none() {
            self.selection = Some(Selection {
                side: Side::Right,
                anchor: 0,
                head: 0,
            });
        }

        self.focus.focus(window, cx);
        cx.notify();
    }
}
