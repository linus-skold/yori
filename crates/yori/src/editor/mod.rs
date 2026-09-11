//! Native aligned editor: viewport rendering, syntax, selection, and input.

use std::{cell::Cell, ops::Range, path::PathBuf, rc::Rc};

mod input;

use yori_document::editing::{EditHistory, EditOutcome, Motion};

use gpui_kit::component::{
    ActiveTheme, ElementExt, Sizable,
    button::{Button, ButtonVariants},
    highlighter::SyntaxHighlighter,
};
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, FocusHandle,
    Focusable, Font, HighlightStyle, InteractiveElement, IntoElement, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Render, ScrollDelta, ScrollWheelEvent,
    SharedString, Styled, StyledText, TextRun, UTF16Selection, UnderlineStyle, Window, canvas, div,
    hsla, point, px,
};
use ropey::Rope;
use yori::{
    display::{DisplayLine, max_display_columns, source_offset_at},
    geometry::{EditorGeometry, display_units, horizontal_scroll_limit, whole_rows},
};
use yori_diff::{Alignment, DiffKind};
use yori_document::Document;

const HEADER_HEIGHT: f32 = 42.0;
const LINE_HEIGHT: f32 = 20.0;
const RESTORE_WIDTH: f32 = 24.0;
const GUTTER_WIDTH: f32 = 58.0 + RESTORE_WIDTH;
const TAB_WIDTH: usize = 4;
const OVERSCAN_ROWS: usize = 4;
const KEY_CONTEXT: &str = "AlignedEditor";

gpui_kit::actions!(
    aligned_editor,
    [
        CopySelected,
        CutSelected,
        Paste,
        SelectAll,
        Undo,
        Redo,
        Backspace,
        Delete,
        Newline,
        InsertTab,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        MoveHome,
        MoveEnd,
        SelectHome,
        SelectEnd,
        MoveStart,
        MoveFinish,
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug)]
struct Selection {
    side: Side,
    anchor: usize,
    head: usize,
}

impl Selection {
    fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

pub(super) struct PaneDocument {
    path: PathBuf,
    max_display_columns: usize,
    document: Document,
    highlighter: Option<SyntaxHighlighter>,
}

impl PaneDocument {
    pub(super) fn new(path: PathBuf, document: Document) -> Self {
        let max_display_columns = max_display_columns(&document, TAB_WIDTH);
        let highlighter = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
            .then(|| {
                let mut highlighter = SyntaxHighlighter::new("rust");
                highlighter.update(None, &Rope::from(document.text()), None);
                highlighter
            });
        Self {
            path,
            max_display_columns,
            document,
            highlighter,
        }
    }

    fn refresh_after_edit(&mut self, edit: &EditOutcome) {
        self.max_display_columns = max_display_columns(&self.document, TAB_WIDTH);
        if let Some(highlighter) = &mut self.highlighter {
            let next = Rope::from(self.document.text());
            let position = |rope: &Rope, offset| {
                let row = rope.byte_to_line_idx(offset, ropey::LineType::LF);
                tree_sitter::Point::new(
                    row,
                    offset - rope.line_to_byte_idx(row, ropey::LineType::LF),
                )
            };
            let new_end = edit.replaced.start + edit.inserted_len;
            let change = tree_sitter::InputEdit {
                start_byte: edit.replaced.start,
                old_end_byte: edit.replaced.end,
                new_end_byte: new_end,
                start_position: position(highlighter.text(), edit.replaced.start),
                old_end_position: position(highlighter.text(), edit.replaced.end),
                new_end_position: position(&next, new_end),
            };
            highlighter.update(Some(change), &next, None);
        }
    }
}

pub(super) struct AlignedEditor {
    left: PaneDocument,
    right: PaneDocument,
    alignment: Alignment,
    history: EditHistory,
    preferred_column: Option<usize>,
    focus: FocusHandle,
    selection: Option<Selection>,
    vertical_scroll: f32,
    horizontal_scroll: f32,
    // Mouse events are window-local; measured bounds provide the editor's content-local inset.
    content_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl AlignedEditor {
    pub(super) fn new(
        left: PaneDocument,
        right: PaneDocument,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let alignment = Alignment::between(&left.document, &right.document);
        Self {
            left,
            right,
            alignment,
            history: EditHistory::default(),
            preferred_column: None,
            focus: cx.focus_handle(),
            selection: None,
            vertical_scroll: 0.0,
            horizontal_scroll: 0.0,
            content_bounds: Rc::new(Cell::new(Bounds::new(
                point(px(0.0), px(0.0)),
                window.viewport_size(),
            ))),
        }
    }

    fn document(&self, side: Side) -> &PaneDocument {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn line_for_row(&self, side: Side, row: usize) -> Option<usize> {
        self.alignment.rows().get(row).and_then(|row| match side {
            Side::Left => row.left,
            Side::Right => row.right,
        })
    }

    fn geometry(&self) -> EditorGeometry {
        let bounds = self.content_bounds.get();
        EditorGeometry::new(
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            HEADER_HEIGHT,
            GUTTER_WIDTH,
            LINE_HEIGHT,
        )
    }

    fn source_offset_at(
        &self,
        position: gpui_kit::Point<Pixels>,
        window: &mut Window,
        cx: &App,
    ) -> (Side, usize) {
        let hit = self.geometry().hit(
            f32::from(position.x),
            f32::from(position.y),
            self.vertical_scroll,
            self.horizontal_scroll,
        );
        let side = if hit.left_side {
            Side::Left
        } else {
            Side::Right
        };
        let pane = self.document(side);
        let row = hit.row;
        if row >= self.alignment.rows().len() {
            return (
                side,
                source_offset_at(
                    &self.alignment,
                    &pane.document,
                    row,
                    side == Side::Left,
                    0,
                    TAB_WIDTH,
                ),
            );
        }

        let Some(line_index) = self.line_for_row(side, row) else {
            return (
                side,
                source_offset_at(
                    &self.alignment,
                    &pane.document,
                    row,
                    side == Side::Left,
                    0,
                    TAB_WIDTH,
                ),
            );
        };
        let source_line = &pane.document.lines()[line_index];
        let display = DisplayLine::from_source(
            pane.document.content(line_index),
            source_line.content.start,
            TAB_WIDTH,
        );
        if hit.text_x <= 0.0 || display.text.is_empty() {
            return (
                side,
                source_offset_at(
                    &self.alignment,
                    &pane.document,
                    row,
                    side == Side::Left,
                    0,
                    TAB_WIDTH,
                ),
            );
        }

        let theme = cx.theme();
        let run = TextRun {
            len: display.text.len(),
            font: Font {
                family: theme.mono_font_family.clone(),
                ..Font::default()
            },
            color: theme.foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(
            display.text.clone().into(),
            theme.mono_font_size,
            &[run],
            None,
        );
        let display_offset = shaped.closest_index_for_x(px(hit.text_x));
        (
            side,
            source_offset_at(
                &self.alignment,
                &pane.document,
                row,
                side == Side::Left,
                display_offset,
                TAB_WIDTH,
            ),
        )
    }

    fn max_horizontal_scroll(&self, window: &mut Window, cx: &App) -> f32 {
        let theme = cx.theme();
        let run = TextRun {
            len: 1,
            font: Font {
                family: theme.mono_font_family.clone(),
                ..Font::default()
            },
            color: theme.foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let cell_width = f32::from(
            window
                .text_system()
                .shape_line(" ".into(), theme.mono_font_size, &[run], None)
                .width(),
        );
        let max_columns = self
            .left
            .max_display_columns
            .max(self.right.max_display_columns);
        horizontal_scroll_limit(
            max_columns,
            cell_width,
            self.geometry().text_viewport_width() - 2.0,
        )
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        self.preferred_column = None;
        self.focus.focus(window, cx);
        let (side, offset) = self.source_offset_at(event.position, window, cx);
        let anchor = self
            .selection
            .as_ref()
            .filter(|selection| event.modifiers.shift && selection.side == side)
            .map_or(offset, |selection| selection.anchor);
        self.selection = Some(Selection {
            side,
            anchor,
            head: offset,
        });
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !event.dragging() {
            return;
        }
        let (side, offset) = self.source_offset_at(event.position, window, cx);
        if let Some(selection) = &mut self.selection
            && selection.side == side
        {
            selection.head = offset;
            cx.notify();
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = match event.delta {
            ScrollDelta::Pixels(delta) => (f32::from(delta.x), f32::from(delta.y)),
            ScrollDelta::Lines(delta) => (delta.x * LINE_HEIGHT, delta.y * LINE_HEIGHT),
        };
        let horizontal = if event.shift {
            delta.0 + delta.1
        } else {
            delta.0
        };
        let vertical = if event.shift { 0.0 } else { delta.1 };
        let max_vertical = self
            .geometry()
            .vertical_scroll_limit(self.alignment.rows().len());
        self.vertical_scroll = (self.vertical_scroll - vertical).clamp(0.0, max_vertical);
        let max_horizontal = self.max_horizontal_scroll(window, cx);
        self.horizontal_scroll = (self.horizontal_scroll - horizontal).clamp(0.0, max_horizontal);
        cx.notify();
        cx.stop_propagation();
    }

    fn copy_selected(&mut self, _: &CopySelected, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = &self.selection else {
            return;
        };
        let range = selection.range();
        if range.is_empty() {
            return;
        }
        let text = self
            .document(selection.side)
            .document
            .copy_range(range)
            .to_owned();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn syntax_highlights(
        pane: &PaneDocument,
        source_range: Range<usize>,
        display: &DisplayLine,
        cx: &Context<Self>,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let Some(highlighter) = &pane.highlighter else {
            return Vec::new();
        };
        highlighter
            .styles(&source_range, cx.theme().highlight_theme.as_ref())
            .into_iter()
            .filter_map(|(range, style)| {
                let display_range = display.display_range(range);
                (!display_range.is_empty()).then_some((display_range, style))
            })
            .collect()
    }

    fn text_highlights(
        &self,
        side: Side,
        pane: &PaneDocument,
        line_index: usize,
        display: &DisplayLine,
        cx: &Context<Self>,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let source_range = pane.document.lines()[line_index].content.clone();
        let syntax = Self::syntax_highlights(pane, source_range.clone(), display, cx);
        let selected = self.selection.as_ref().and_then(|selection| {
            if selection.side != side {
                return None;
            }
            let range = selection.range();
            let overlap = range.start.max(source_range.start)..range.end.min(source_range.end);
            (overlap.start < overlap.end).then(|| display.display_range(overlap))
        });
        let marked = (side == Side::Right)
            .then(|| self.history.marked_range())
            .flatten()
            .map(|range| display.display_range(range))
            .filter(|range| !range.is_empty());
        let mut boundaries = vec![0, display.text.len()];
        for (range, _) in &syntax {
            boundaries.extend([range.start, range.end]);
        }
        if let Some(range) = &selected {
            boundaries.extend([range.start, range.end]);
        }
        if let Some(range) = &marked {
            boundaries.extend([range.start, range.end]);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries
            .windows(2)
            .filter_map(|pair| {
                let range = pair[0]..pair[1];
                if range.is_empty() {
                    return None;
                }
                let mut style = syntax
                    .iter()
                    .find(|(syntax_range, _)| {
                        syntax_range.start <= range.start && syntax_range.end >= range.end
                    })
                    .map(|(_, style)| *style)
                    .unwrap_or_default();
                if selected.as_ref().is_some_and(|selected| {
                    selected.start < range.end && selected.end > range.start
                }) {
                    style.background_color = Some(cx.theme().selection);
                }
                if marked
                    .as_ref()
                    .is_some_and(|marked| marked.start < range.end && marked.end > range.start)
                {
                    style.underline = Some(UnderlineStyle {
                        color: Some(cx.theme().foreground),
                        thickness: px(1.0),
                        wavy: false,
                    });
                }
                Some((range, style))
            })
            .collect()
    }

    fn row_background(kind: DiffKind, side: Side) -> gpui_kit::Hsla {
        match (kind, side) {
            (DiffKind::Equal, _) => hsla(0.0, 0.0, 0.0, 0.0),
            (DiffKind::Removed, Side::Left) => hsla(0.0, 0.62, 0.22, 0.72),
            (DiffKind::Added, Side::Right) => hsla(0.34, 0.48, 0.18, 0.72),
            (DiffKind::Modified, Side::Left) => hsla(0.07, 0.58, 0.22, 0.72),
            (DiffKind::Modified, Side::Right) => hsla(0.14, 0.48, 0.20, 0.72),
            _ => hsla(0.0, 0.0, 0.08, 0.42),
        }
    }

    fn render_pane_row(
        &self,
        side: Side,
        row_index: usize,
        top: f32,
        pane_width: f32,
        text_viewport_width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let row = &self.alignment.rows()[row_index];
        let pane = self.document(side);
        let line = match side {
            Side::Left => row.left,
            Side::Right => row.right,
        };
        let background = Self::row_background(row.kind, side);
        let mut container = div()
            .absolute()
            .top(px(top))
            .left(px(if side == Side::Left { 0.0 } else { pane_width }))
            .w(px(pane_width))
            .h(px(LINE_HEIGHT))
            .overflow_hidden()
            .bg(background);

        if let Some(line_index) = line {
            let source_line = &pane.document.lines()[line_index];
            let display = DisplayLine::from_source(
                pane.document.content(line_index),
                source_line.content.start,
                TAB_WIDTH,
            );
            let highlights = self.text_highlights(side, pane, line_index, &display, cx);
            let text =
                StyledText::new(SharedString::from(display.text)).with_highlights(highlights);
            container = container
                .child(
                    div()
                        .absolute()
                        .left(px(RESTORE_WIDTH))
                        .w(px(GUTTER_WIDTH - RESTORE_WIDTH))
                        .h(px(LINE_HEIGHT))
                        .overflow_hidden()
                        .text_right()
                        .pr(px(8.0))
                        .text_color(cx.theme().muted_foreground)
                        .child((line_index + 1).to_string()),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(GUTTER_WIDTH))
                        .w(px(text_viewport_width))
                        .h(px(LINE_HEIGHT))
                        .overflow_hidden()
                        .child(
                            div()
                                .absolute()
                                .left(px(-self.horizontal_scroll))
                                .whitespace_nowrap()
                                .child(text),
                        ),
                );
        }
        container
    }
}

impl Focusable for AlignedEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AlignedEditor {
    #[expect(
        clippy::too_many_lines,
        reason = "one declarative GPUI widget tree keeps input bindings, clipping, and overlays in their rendering order"
    )]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let geometry = self.geometry();
        let width = geometry.pane_width() * 2.0;
        let pane_width = geometry.pane_width();
        let text_viewport_width = geometry.text_viewport_width();
        self.horizontal_scroll = self
            .horizontal_scroll
            .min(self.max_horizontal_scroll(window, cx));
        self.vertical_scroll = self
            .vertical_scroll
            .min(geometry.vertical_scroll_limit(self.alignment.rows().len()));
        let first_row = whole_rows(self.vertical_scroll / LINE_HEIGHT);
        let row_offset = self.vertical_scroll % LINE_HEIGHT;
        let visible_count =
            whole_rows((geometry.rows_viewport_height() / LINE_HEIGHT).ceil()) + OVERSCAN_ROWS;
        let end_row = (first_row + visible_count).min(self.alignment.rows().len());

        let mut rows = div()
            .id("rows-viewport")
            .absolute()
            .top(px(HEADER_HEIGHT))
            .left(px(0.0))
            .w(px(width))
            .h(px(geometry.rows_viewport_height()))
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move));
        for row_index in first_row..end_row {
            let top = geometry.visible_row_top(row_index, first_row, row_offset);
            rows = rows
                .child(self.render_pane_row(
                    Side::Left,
                    row_index,
                    top,
                    pane_width,
                    text_viewport_width,
                    cx,
                ))
                .child(self.render_pane_row(
                    Side::Right,
                    row_index,
                    top,
                    pane_width,
                    text_viewport_width,
                    cx,
                ));
        }

        if self.focus.is_focused(window)
            && let Some(selection) = self.right_selection()
        {
            let (row, x) = self.cursor_position(selection.head, window, cx);
            rows = rows.child(
                div()
                    .absolute()
                    .left(px(pane_width + GUTTER_WIDTH))
                    .top(px(0.0))
                    .w(px(text_viewport_width))
                    .h(px(geometry.rows_viewport_height()))
                    .overflow_hidden()
                    .child(
                        div()
                            .absolute()
                            .left(px(x - self.horizontal_scroll))
                            .top(px(display_units(row) * LINE_HEIGHT - self.vertical_scroll))
                            .w(px(1.0))
                            .h(px(LINE_HEIGHT))
                            .bg(cx.theme().foreground),
                    ),
            );
        }
        // One control per changed run, including runs with no baseline lines.
        // Keep a tall block's control reachable when its first row scrolls away.
        let first_block = self
            .alignment
            .blocks()
            .partition_point(|b| b.rows.end <= first_row);
        for (index, block) in self.alignment.blocks().iter().enumerate().skip(first_block) {
            if block.rows.start >= end_row {
                break;
            }
            let top = ((display_units(block.rows.start) * LINE_HEIGHT - self.vertical_scroll)
                .max(0.0))
            .min(display_units(block.rows.end) * LINE_HEIGHT - self.vertical_scroll - LINE_HEIGHT);
            let expected = block.clone();
            rows = rows.child(
                div()
                    .absolute()
                    .left(px(pane_width + 1.0))
                    .top(px(top))
                    .w(px(RESTORE_WIDTH - 2.0))
                    .h(px(LINE_HEIGHT))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_move(|_, _, cx| cx.stop_propagation())
                    .child(
                        Button::new(("restore-block", index))
                            .label("→")
                            .ghost()
                            .compact()
                            .with_size(px(LINE_HEIGHT))
                            .w(px(RESTORE_WIDTH - 2.0))
                            .tooltip("Restore this block from the left (undo: Ctrl+Z)")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.restore_block(index, &expected, window, cx);
                            })),
                    ),
            );
        }
        let input_entity = cx.entity();
        let input_focus = self.focus.clone();
        let measured_content_bounds = Rc::clone(&self.content_bounds);
        div()
            .id("aligned-editor")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .overflow_hidden()
            .cursor_text()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(cx.theme().mono_font_size)
            .line_height(px(LINE_HEIGHT))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::copy_selected))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::insert_tab))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(|this, _: &MoveLeft, w, cx| {
                this.move_cursor(Motion::Left, false, w, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveRight, w, cx| {
                this.move_cursor(Motion::Right, false, w, cx);
            }))
            .on_action(
                cx.listener(|this, _: &MoveUp, w, cx| this.move_cursor(Motion::Up, false, w, cx)),
            )
            .on_action(cx.listener(|this, _: &MoveDown, w, cx| {
                this.move_cursor(Motion::Down, false, w, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLeft, w, cx| {
                this.move_cursor(Motion::Left, true, w, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectRight, w, cx| {
                this.move_cursor(Motion::Right, true, w, cx);
            }))
            .on_action(
                cx.listener(|this, _: &SelectUp, w, cx| this.move_cursor(Motion::Up, true, w, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectDown, w, cx| {
                this.move_cursor(Motion::Down, true, w, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveHome, w, cx| {
                this.move_cursor(Motion::Home, false, w, cx);
            }))
            .on_action(
                cx.listener(|this, _: &MoveEnd, w, cx| this.move_cursor(Motion::End, false, w, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectHome, w, cx| {
                this.move_cursor(Motion::Home, true, w, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectEnd, w, cx| {
                this.move_cursor(Motion::End, true, w, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveStart, w, cx| {
                this.move_cursor(Motion::Start, false, w, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveFinish, w, cx| {
                this.move_cursor(Motion::Finish, false, w, cx);
            }))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .on_prepaint(move |bounds, window, _| {
                if measured_content_bounds.replace(bounds) != bounds {
                    window.refresh();
                }
            })
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        if input_entity.read(cx).right_selection().is_some() {
                            window.handle_input(
                                &input_focus,
                                ElementInputHandler::new(bounds, input_entity.clone()),
                                cx,
                            );
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(rows)
            .child(
                div()
                    .absolute()
                    .top(px(HEADER_HEIGHT))
                    .left(px(pane_width))
                    .w(px(1.0))
                    .h(px(geometry.rows_viewport_height()))
                    .bg(cx.theme().border),
            )
            .child(
                div()
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
                    .w(px(pane_width))
                    .h(px(HEADER_HEIGHT))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(format!("{} — read-only", self.left.path.display())),
            )
            .child(
                div()
                    .absolute()
                    .top(px(0.0))
                    .left(px(pane_width))
                    .w(px(pane_width))
                    .h(px(HEADER_HEIGHT))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .border_b_1()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(format!(
                        "{} — editable · memory only",
                        self.right.path.display()
                    )),
            )
    }
}

pub(super) fn init(cx: &mut App) {
    input::bind_keys(cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::highlighter::HighlightTheme;
    use yori_document::editing::TextSelection;

    fn pane(text: &str) -> PaneDocument {
        PaneDocument::new(
            PathBuf::from("fixture.rs"),
            Document::from_bytes(text.as_bytes().to_vec()).unwrap(),
        )
    }

    #[test]
    fn syntax_refresh_matches_a_fresh_parse_after_edit_and_undo() {
        let mut pane = pane("fn greet() {\n    let name = \"hello\";\n}\n");
        let original = pane.document.text().to_owned();
        let mut history = EditHistory::default();
        let offset = original.find("hello").unwrap();
        let selection = TextSelection {
            anchor: offset,
            head: offset + 5,
        };
        let edit = history
            .replace(
                &mut pane.document,
                selection,
                selection.range(),
                "界\\nworld",
            )
            .unwrap();
        pane.refresh_after_edit(&edit);
        let fresh = PaneDocument::new(pane.path.clone(), pane.document.clone());
        let theme = HighlightTheme::default_dark();
        let range = 0..pane.document.text().len();
        let expected = fresh
            .highlighter
            .as_ref()
            .unwrap()
            .styles(&range, theme.as_ref());
        assert!(!expected.is_empty());
        assert_eq!(
            pane.highlighter
                .as_ref()
                .unwrap()
                .styles(&range, theme.as_ref()),
            expected
        );
        let undo = history
            .undo(&mut pane.document, edit.selection)
            .unwrap()
            .unwrap();
        pane.refresh_after_edit(&undo);
        assert_eq!(pane.document.text(), original);
        assert_eq!(
            pane.highlighter.as_ref().unwrap().text().to_string(),
            original
        );
    }

    #[test]
    #[ignore = "explicit 5k editing latency measurement"]
    fn five_thousand_line_edit_refresh_and_realign() {
        use std::fmt::Write as _;
        let mut source = String::new();
        for line in 0..5_000 {
            writeln!(source, "fn row_{line}() -> usize {{ {line} }}").unwrap();
        }
        let left = Document::from_bytes(source.as_bytes().to_vec()).unwrap();
        let mut pane = pane(&source);
        let mut history = EditHistory::default();
        let started = std::time::Instant::now();
        for _ in 0..10 {
            let edit = history
                .replace(
                    &mut pane.document,
                    TextSelection::caret(0),
                    0..0,
                    "// note\n",
                )
                .unwrap();
            pane.refresh_after_edit(&edit);
            let alignment = Alignment::between(&left, &pane.document);
            assert_eq!(alignment.rows().len(), 5_001);
            let undo = history
                .undo(&mut pane.document, edit.selection)
                .unwrap()
                .unwrap();
            pane.refresh_after_edit(&undo);
            assert_eq!(
                Alignment::between(&left, &pane.document).rows().len(),
                5_000
            );
        }
        eprintln!(
            "5k: 20 edit/syntax/realignment operations in {:?}",
            started.elapsed()
        );
        assert_eq!(pane.document.text(), source);
    }
}
