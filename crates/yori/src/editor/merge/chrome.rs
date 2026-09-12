//! Read-only ancestor context inside the aligned merge surface.

use gpui_kit::component::ActiveTheme;
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div, px};
use yori::{display::DisplayLine, geometry::EditorGeometry};

use super::{AlignedEditor, BaseRow, LINE_HEIGHT};
use crate::editor::{GUTTER_WIDTH, TAB_WIDTH};

impl AlignedEditor {
    pub(in crate::editor) fn render_base_preview_row(
        &self,
        row: &BaseRow,
        top: f32,
        geometry: EditorGeometry,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let merge = self.merge.as_ref().expect("merge mode");
        let source_line = match row {
            BaseRow::SourceLine(line) => Some(*line),
            BaseRow::Caption | BaseRow::EmptyAncestor => None,
        };
        let text = match row {
            BaseRow::Caption => "BASE · Common ancestor · Read-only".to_owned(),
            BaseRow::EmptyAncestor => "(Empty ancestor)".to_owned(),
            BaseRow::SourceLine(line) => {
                DisplayLine::from_source(merge.session.base().content(*line), 0, TAB_WIDTH).text
            }
        };
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
