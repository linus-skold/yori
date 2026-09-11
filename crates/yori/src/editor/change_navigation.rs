//! Wire change navigation into the existing caret, focus and viewport model.

use super::{AlignedEditor, NextChange, PreviousChange, Selection, Side};
use gpui_kit::{Context, Pixels, Point, Window};
use yori::navigation::ChangeDirection;

impl AlignedEditor {
    pub(super) fn locate_pointer_change(&mut self, position: Point<Pixels>) {
        let row = self
            .geometry()
            .hit(
                f32::from(position.x),
                f32::from(position.y),
                self.vertical_scroll,
                self.horizontal_scroll,
            )
            .row;
        self.navigation.locate(row);
    }

    pub(super) fn locate_caret_change(&mut self) {
        if let Some(selection) = &self.selection {
            let row = self.alignment.row_for_offset(
                &self.document(selection.side).document,
                selection.head,
                selection.side == Side::Left,
            );
            self.navigation.locate(row);
        }
    }

    pub(super) fn previous_change(
        &mut self,
        _: &PreviousChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_change(ChangeDirection::Previous, window, cx);
    }

    pub(super) fn next_change(
        &mut self,
        _: &NextChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_change(ChangeDirection::Next, window, cx);
    }

    fn navigate_change(
        &mut self,
        direction: ChangeDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_vim();
        self.finish_composition();
        let Some(target) = self.navigation.advance(&self.alignment, direction) else {
            return;
        };

        self.selection = Some(Selection {
            side: Side::Right,
            anchor: target.right_offset,
            head: target.right_offset,
        });
        self.preferred_column = None;
        self.horizontal_scroll = 0.0;
        self.vertical_scroll = self
            .geometry()
            .change_scroll_top(target.rows.start, self.alignment.rows().len());

        self.focus.focus(window, cx);
        cx.notify();
    }
}
