//! GPUI input integration for the same aligned surface used by the read-only checkpoint.
use super::{
    ActiveTheme, AlignedEditor, Alignment, App, Backspace, Bounds, Context, CopySelected,
    CutSelected, Delete, DisplayLine, EditOutcome, EntityInputHandler, Font, GUTTER_WIDTH,
    HEADER_HEIGHT, InsertTab, KEY_CONTEXT, KeyBinding, LINE_HEIGHT, Motion, MoveDown, MoveEnd,
    MoveFinish, MoveHome, MoveLeft, MoveRight, MoveStart, MoveUp, Newline, NextChange, Paste,
    Pixels, PreviousChange, Range, Redo, SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft,
    SelectRight, SelectUp, Selection, Side, TAB_WIDTH, TextRun, UTF16Selection, Undo, Window,
    point, px, source_offset_at,
};
use yori::geometry::{display_units, whole_rows};
use yori_document::editing::{self, TextSelection};

struct ViewAnchor {
    side: Side,
    offset: usize,
    fraction: f32,
}

impl AlignedEditor {
    pub(super) fn right_selection(&self) -> Option<TextSelection> {
        self.selection
            .as_ref()
            .filter(|s| s.side == Side::Right)
            .map(|s| TextSelection {
                anchor: s.anchor,
                head: s.head,
            })
    }

    pub(super) fn finish_composition(&mut self) {
        if let Some(selection) = self.right_selection() {
            self.history
                .finish_composition(&self.right.document, selection);
        }
    }

    fn view_anchor(&self) -> ViewAnchor {
        let row = whole_rows(self.vertical_scroll / LINE_HEIGHT);
        let side = if self.line_for_row(Side::Left, row).is_some() {
            Side::Left
        } else {
            Side::Right
        };
        let document = &self.document(side).document;
        ViewAnchor {
            side,
            offset: source_offset_at(
                &self.alignment,
                document,
                row,
                side == Side::Left,
                0,
                TAB_WIDTH,
            ),
            fraction: self.vertical_scroll % LINE_HEIGHT,
        }
    }

    fn finish_edit(
        &mut self,
        anchor: &ViewAnchor,
        edit: &EditOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.right.refresh_after_edit(edit);
        self.alignment = Alignment::between(&self.left.document, &self.right.document);
        self.selection = Some(Selection {
            side: Side::Right,
            anchor: edit.selection.anchor,
            head: edit.selection.head,
        });
        self.preferred_column = None;
        let offset = if anchor.side == Side::Right {
            edit.map_anchor(anchor.offset)
        } else {
            anchor.offset
        };
        let row = self.alignment.row_for_offset(
            &self.document(anchor.side).document,
            offset,
            anchor.side == Side::Left,
        );
        self.vertical_scroll = display_units(row) * LINE_HEIGHT + anchor.fraction;
        self.locate_caret_change();
        self.reveal_cursor(window, cx);
        cx.notify();
    }

    pub(super) fn restore_block(
        &mut self,
        index: usize,
        expected: &yori_diff::ChangeBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Never apply coordinates from a button rendered for a different alignment.
        if self.alignment.blocks().get(index) != Some(expected) {
            return;
        }
        self.finish_composition();
        let selection = self
            .right_selection()
            .unwrap_or(TextSelection::caret(expected.right.start));
        let anchor = self.view_anchor();
        match yori_diff::restore_block(
            &mut self.history,
            &self.left.document,
            &mut self.right.document,
            selection,
            expected,
        ) {
            Ok(edit) => {
                self.focus.focus(window, cx);
                self.finish_edit(&anchor, &edit, window, cx);
            }
            Err(error) => {
                eprintln!("block restoration rejected: {error}");
                window.play_system_bell();
            }
        }
    }

    fn replace(
        &mut self,
        range: Range<usize>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        let anchor = self.view_anchor();
        match self
            .history
            .replace(&mut self.right.document, selection, range, text)
        {
            Ok(edit) => self.finish_edit(&anchor, &edit, window, cx),
            Err(error) => {
                eprintln!("edit rejected: {error}");
                window.play_system_bell();
            }
        }
    }

    pub(super) fn cursor_position(
        &self,
        offset: usize,
        window: &mut Window,
        cx: &App,
    ) -> (usize, f32) {
        let document = &self.right.document;
        let row = self.alignment.row_for_offset(document, offset, false);
        let range = document.line_content_range(document.line_at_offset(offset));
        let display =
            DisplayLine::from_source(&document.text()[range.clone()], range.start, TAB_WIDTH);
        if display.text.is_empty() {
            return (row, 0.0);
        }
        let display_offset = display.display_offset(offset);
        let run = TextRun {
            len: display.text.len(),
            font: Font {
                family: cx.theme().mono_font_family.clone(),
                ..Font::default()
            },
            color: cx.theme().foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            display.text.into(),
            cx.theme().mono_font_size,
            &[run],
            None,
        );
        (row, f32::from(line.x_for_index(display_offset)))
    }

    fn reveal_cursor(&mut self, window: &mut Window, cx: &App) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        let (row, x) = self.cursor_position(selection.head, window, cx);
        let geometry = self.geometry();
        let y = display_units(row) * LINE_HEIGHT;
        let height = geometry.rows_viewport_height();
        if y < self.vertical_scroll {
            self.vertical_scroll = y;
        } else if y + LINE_HEIGHT > self.vertical_scroll + height {
            self.vertical_scroll = (y + LINE_HEIGHT - height).max(0.0);
        }
        self.vertical_scroll = self
            .vertical_scroll
            .min(geometry.vertical_scroll_limit(self.alignment.rows().len()));
        let width = geometry.text_viewport_width();
        if x < self.horizontal_scroll {
            self.horizontal_scroll = x;
        } else if x + 2.0 > self.horizontal_scroll + width {
            self.horizontal_scroll = (x + 2.0 - width).max(0.0);
        }
    }

    pub(super) fn move_cursor(
        &mut self,
        motion: Motion,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_composition();
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let side = selection.side;
        let old = TextSelection {
            anchor: selection.anchor,
            head: selection.head,
        };
        let document = if side == Side::Left {
            &self.left.document
        } else {
            &self.right.document
        };
        let next = editing::navigate(document, old, motion, extend, &mut self.preferred_column);
        self.selection = Some(Selection {
            side,
            anchor: next.anchor,
            head: next.head,
        });
        self.locate_caret_change();
        self.reveal_cursor(window, cx);
        cx.notify();
    }

    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        let Some(side) = self.selection.as_ref().map(|s| s.side) else {
            return;
        };
        self.selection = Some(Selection {
            side,
            anchor: 0,
            head: self.document(side).document.text().len(),
        });
        self.locate_caret_change();
        self.preferred_column = None;
        cx.notify();
    }

    pub(super) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let range = self.history.marked_range().unwrap_or(selection.range());
            self.replace(range, &text, window, cx);
        }
    }

    pub(super) fn cut(&mut self, _: &CutSelected, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        if !selection.range().is_empty() {
            self.copy_selected(&CopySelected, window, cx);
            self.replace(selection.range(), "", window, cx);
        }
    }

    pub(super) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_at_cursor(true, window, cx);
    }

    pub(super) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_at_cursor(false, window, cx);
    }

    fn delete_at_cursor(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        let mut range = selection.range();
        if range.is_empty() {
            if backwards {
                range.start = editing::previous_grapheme(self.right.document.text(), range.start);
            } else {
                range.end = editing::next_grapheme(self.right.document.text(), range.end);
            }
        }
        if !range.is_empty() {
            self.replace(range, "", window, cx);
        }
    }

    pub(super) fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        self.replace(selection.range(), self.right.document.newline(), window, cx);
    }

    pub(super) fn insert_tab(
        &mut self,
        _: &InsertTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        self.replace(selection.range(), "\t", window, cx);
    }

    pub(super) fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        self.travel_history(false, window, cx);
    }

    pub(super) fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        self.travel_history(true, window, cx);
    }

    fn travel_history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        let anchor = self.view_anchor();
        let result = if redo {
            self.history.redo(&mut self.right.document, selection)
        } else {
            self.history.undo(&mut self.right.document, selection)
        };
        match result {
            Ok(Some(edit)) => self.finish_edit(&anchor, &edit, window, cx),
            Ok(None) => {}
            Err(error) => {
                eprintln!("history edit rejected: {error}");
                window.play_system_bell();
            }
        }
    }

    fn bytes_from_utf16(&self, range: Range<usize>) -> Range<usize> {
        let text = self.right.document.text();
        editing::from_utf16(text, range.start)..editing::from_utf16(text, range.end)
    }

    fn bytes_to_utf16(&self, range: Range<usize>) -> Range<usize> {
        let text = self.right.document.text();
        editing::to_utf16(text, range.start)..editing::to_utf16(text, range.end)
    }
}

impl EntityInputHandler for AlignedEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        self.right_selection()?;
        let bytes = self.bytes_from_utf16(range);
        *actual = Some(self.bytes_to_utf16(bytes.clone()));
        self.right.document.text().get(bytes).map(str::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.right_selection()?;
        Some(UTF16Selection {
            range: self.bytes_to_utf16(selection.range()),
            reversed: selection.head < selection.anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.right_selection()?;
        self.history
            .marked_range()
            .map(|range| self.bytes_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        let range = range
            .map(|range| self.bytes_from_utf16(range))
            .or_else(|| self.history.marked_range())
            .unwrap_or(selection.range());
        self.replace(range, text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.right_selection() else {
            return;
        };
        let range = range
            .map(|range| self.bytes_from_utf16(range))
            .or_else(|| self.history.marked_range())
            .unwrap_or(selection.range());
        let selected = selected.map(|range| {
            editing::from_utf16(text, range.start)..editing::from_utf16(text, range.end)
        });
        let anchor = self.view_anchor();
        match self.history.replace_marked(
            &mut self.right.document,
            selection,
            range,
            text,
            selected,
        ) {
            Ok(edit) => self.finish_edit(&anchor, &edit, window, cx),
            Err(error) => {
                eprintln!("composition rejected: {error}");
                window.play_system_bell();
            }
        }
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.right_selection()?;
        let range = self.bytes_from_utf16(range);
        let (row, x) = self.cursor_position(range.start, window, cx);
        let (end_row, end_x) = self.cursor_position(range.end, window, cx);
        let origin = self.content_bounds.get().origin;
        let width = if row == end_row {
            (end_x - x).max(1.0)
        } else {
            1.0
        };
        Some(Bounds::new(
            point(
                origin.x
                    + px(self.geometry().pane_width() + GUTTER_WIDTH + x - self.horizontal_scroll),
                origin.y
                    + px(HEADER_HEIGHT + display_units(row) * LINE_HEIGHT - self.vertical_scroll),
            ),
            gpui_kit::size(px(width), px(LINE_HEIGHT)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui_kit::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.right_selection()?;
        let (side, offset) = self.source_offset_at(point, window, cx);
        (side == Side::Right).then(|| editing::to_utf16(self.right.document.text(), offset))
    }
}

pub(super) fn bind_keys(cx: &mut App) {
    let command = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new("alt-up", PreviousChange, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-down", NextChange, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-c"), CopySelected, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-v"), Paste, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-x"), CutSelected, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-a"), SelectAll, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-z"), Undo, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-shift-z"), Redo, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-y"), Redo, Some(KEY_CONTEXT)),
        KeyBinding::new("backspace", Backspace, Some(KEY_CONTEXT)),
        KeyBinding::new("delete", Delete, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Newline, Some(KEY_CONTEXT)),
        KeyBinding::new("tab", InsertTab, Some(KEY_CONTEXT)),
        KeyBinding::new("left", MoveLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("right", MoveRight, Some(KEY_CONTEXT)),
        KeyBinding::new("up", MoveUp, Some(KEY_CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(KEY_CONTEXT)),
        KeyBinding::new("home", MoveHome, Some(KEY_CONTEXT)),
        KeyBinding::new("end", MoveEnd, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-home", SelectHome, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-end", SelectEnd, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-home", MoveStart, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-end", MoveFinish, Some(KEY_CONTEXT)),
    ]);
}
