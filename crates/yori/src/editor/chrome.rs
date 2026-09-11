//! Compact standard controls around the custom diff surface.

use std::path::Path;

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
};
use gpui_kit::{Context, FontWeight, IntoElement, ParentElement, Styled, div, px};
use yori::{geometry::display_units, navigation::ChangeDirection};

use super::{AlignedEditor, HEADER_HEIGHT, NextChange, PreviousChange, Side};

fn path_labels(path: &Path) -> (String, String) {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let directory = parent
        .unwrap_or_else(|| Path::new("."))
        .display()
        .to_string();

    (name, directory)
}

impl AlignedEditor {
    fn render_review_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.alignment.blocks().len();
        let current = self
            .navigation
            .current(&self.alignment)
            .map_or(0, |index| index + 1);
        let label = format!("{current} of {count}");

        // Reserve both numbers at the total's digit count, including the 9 → 10 transition.
        let counter_width = 24.0 + 16.0 * display_units(count.to_string().len());
        let restore = self.selection_restore();

        div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .gap(px(8.0))
            .text_size(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .flex_shrink_0()
                    .child(
                        Button::new("previous-change")
                            .icon(IconName::ArrowUp)
                            .accessibility_label("Previous change")
                            .ghost()
                            .with_size(px(28.0))
                            .tooltip("Previous change (Alt+Up)")
                            .disabled(
                                self.navigation
                                    .target(&self.alignment, ChangeDirection::Previous)
                                    .is_none(),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.previous_change(&PreviousChange, window, cx);
                            })),
                    )
                    .child(
                        Button::new("next-change")
                            .icon(IconName::ArrowDown)
                            .accessibility_label("Next change")
                            .ghost()
                            .with_size(px(28.0))
                            .tooltip("Next change (Alt+Down)")
                            .disabled(
                                self.navigation
                                    .target(&self.alignment, ChangeDirection::Next)
                                    .is_none(),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.next_change(&NextChange, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .w(px(counter_width))
                    .flex_shrink_0()
                    .text_right()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .children(restore.map(|plan| {
                let tooltip = format!(
                    "{} (Alt+Enter; undo: Ctrl+Z)",
                    self.restore_description(&plan)
                );

                Button::new("restore-selected-lines")
                    .icon(IconName::ArrowRight)
                    .label("Restore lines")
                    .ghost()
                    .small()
                    .tooltip(tooltip)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.apply_selection_restore(&plan, window, cx);
                    }))
            }))
    }

    pub(super) fn render_pane_header(
        &self,
        side: Side,
        pane_width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (name, directory) = path_labels(&self.document(side).path);

        div()
            .absolute()
            .top(px(0.0))
            .left(px(if side == Side::Left {
                0.0
            } else {
                self.geometry().right_pane_left()
            }))
            .w(px(if side == Side::Right {
                pane_width + super::scrollbar::WIDTH
            } else {
                pane_width
            }))
            .h(px(HEADER_HEIGHT))
            .px(px(16.0))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(4.0))
            .overflow_hidden()
            .cursor_default()
            .font_family(cx.theme().font_family.clone())
            .text_size(px(13.0))
            .line_height(px(18.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                div()
                    .h(px(28.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        Icon::new(IconName::FileText)
                            .with_size(px(16.0))
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .children((side == Side::Left).then(|| {
                        div()
                            .flex_shrink_0()
                            .text_size(px(12.0))
                            .text_color(cx.theme().muted_foreground)
                            .child("Baseline · Read-only")
                    }))
                    .children((side == Side::Right).then(|| self.render_review_controls(cx))),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(directory),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::path_labels;
    use crate::editor::{
        AlignedEditor, GUTTER_WIDTH, HEADER_HEIGHT, LINE_HEIGHT, PaneDocument, TEXT_INSET,
    };
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, TestAppContext};
    use std::{fmt::Write as _, path::Path};
    use yori::geometry::EditorGeometry;
    use yori_document::Document;

    #[gpui_kit::test]
    fn navigation_keeps_arrow_positions_stable_across_counter_changes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::appearance::init(cx);
            crate::editor::init(cx);
        });

        let (_, cx) = cx.add_window_view(|window, cx| {
            let pane = |name: &str| {
                let mut text = String::new();
                for index in 0..12 {
                    writeln!(text, "unchanged {index}\n{name} {index}").unwrap();
                }

                let document = Document::from_bytes(text.into_bytes()).unwrap();

                PaneDocument::new(name.into(), document)
            };
            let editor = cx.new(|cx| AlignedEditor::new(pane("left"), pane("right"), window, cx));

            Root::new(editor, window, cx)
        });
        cx.update(TestWindowExt::render_frame);
        cx.run_until_parked();

        let initial = cx.update(|window, _| window.find("next-change").bounds());
        for _ in 0..12 {
            cx.update(|window, cx| window.click("next-change", cx));
            cx.run_until_parked();

            cx.update(|window, cx| {
                window.render_frame(cx);
                assert_eq!(window.find("next-change").bounds(), initial);
            });
        }
    }

    #[test]
    fn header_and_type_spacing_share_the_editors_hit_geometry() {
        let geometry = EditorGeometry::new(
            15.0,
            25.0,
            1200.0,
            800.0,
            HEADER_HEIGHT,
            GUTTER_WIDTH,
            LINE_HEIGHT,
        );
        let text_x = 15.0 + geometry.pane_width() + GUTTER_WIDTH;
        let text_y = 25.0 + HEADER_HEIGHT;

        let first = geometry.hit(text_x, text_y, 0.0, 0.0);
        let scrolled = geometry.hit(text_x, text_y, LINE_HEIGHT, 0.0);

        assert!(!first.left_side);
        assert!(!first.in_gutter);
        assert_eq!(first.row, 0);
        assert_eq!(scrolled.row, 1);
        assert!(geometry.hit(text_x - 1.0, text_y, 0.0, 0.0).in_gutter);
    }

    #[test]
    fn text_inset_stays_fixed_while_source_content_scrolls() {
        let geometry = EditorGeometry::new(
            15.0,
            25.0,
            1200.0,
            800.0,
            HEADER_HEIGHT,
            GUTTER_WIDTH,
            LINE_HEIGHT,
        );

        for pane_offset in [0.0, geometry.pane_width()] {
            let text_x = 15.0 + pane_offset + GUTTER_WIDTH;
            let text_y = 25.0 + HEADER_HEIGHT;
            let inset = geometry.hit(text_x - TEXT_INSET / 2.0, text_y, 0.0, 40.0);
            let text = geometry.hit(text_x, text_y, 0.0, 40.0);

            assert!(inset.in_gutter);
            assert!(inset.text_x.abs() < f32::EPSILON);
            assert!(!text.in_gutter);
            assert!((text.text_x - 40.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn headers_separate_file_identity_from_directory_metadata() {
        assert_eq!(
            path_labels(Path::new("parser.rs")),
            ("parser.rs".into(), ".".into())
        );
        assert_eq!(
            path_labels(Path::new("/project/源/parser.rs")),
            ("parser.rs".into(), "/project/源".into())
        );
        assert_eq!(
            path_labels(Path::new("../src/config.go")),
            ("config.go".into(), "../src".into())
        );
    }
}
