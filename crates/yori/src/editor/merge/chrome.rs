//! Read-only ancestor context inside the aligned merge surface.

use gpui_kit::component::ActiveTheme;
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div, px};
use yori::{display::DisplayLine, geometry::EditorGeometry};

use super::{AlignedEditor, LINE_HEIGHT};
use crate::editor::{GUTTER_WIDTH, TAB_WIDTH};

impl AlignedEditor {
    pub(in crate::editor) fn is_base_preview_row(&self, row: usize) -> bool {
        self.merge
            .as_ref()
            .and_then(|merge| merge.base_preview.as_ref())
            .is_some_and(|(rows, _)| rows.contains(&row))
    }

    pub(in crate::editor) fn render_base_preview_row(
        &self,
        row: usize,
        top: f32,
        geometry: EditorGeometry,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let merge = self.merge.as_ref().expect("merge mode");
        let (rows, lines) = merge.base_preview.as_ref().expect("base preview row");
        let offset = row - rows.start;
        let source_line = (offset > 0 && !lines.is_empty()).then(|| lines.start + offset - 1);
        let text = source_line.map_or_else(
            || {
                if offset == 0 {
                    "BASE · Common ancestor · Read-only"
                } else {
                    "(Empty ancestor)"
                }
                .to_owned()
            },
            |line| DisplayLine::from_source(merge.session.base().content(line), 0, TAB_WIDTH).text,
        );
        let number = source_line.map_or_else(String::new, |line| (line + 1).to_string());

        div()
            .absolute()
            .top(px(top))
            .left(px(geometry.right_pane_left()))
            .w(px(geometry.pane_width()))
            .h(px(LINE_HEIGHT))
            .overflow_hidden()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .w(px(GUTTER_WIDTH - 10.0))
                    .text_right()
                    .child(number),
            )
            .child(
                div()
                    .absolute()
                    .left(px(GUTTER_WIDTH))
                    .w(px(geometry.text_viewport_width()))
                    .h(px(LINE_HEIGHT))
                    .overflow_hidden()
                    .child(
                        div()
                            .absolute()
                            .left(px(if source_line.is_some() {
                                -self.horizontal_scroll
                            } else {
                                0.0
                            }))
                            .whitespace_nowrap()
                            .child(text),
                    ),
            )
    }
}
