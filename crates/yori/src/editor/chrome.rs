//! Compact standard controls around the custom diff surface.

use std::path::Path;

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    switch::Switch,
};
use gpui_kit::{Context, FontWeight, IntoElement, ParentElement, Styled, div, px};
use yori::navigation::ChangeDirection;

use super::{AlignedEditor, FILE_HEADER_HEIGHT, NextChange, PreviousChange, Side, TOOLBAR_HEIGHT};

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
    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.alignment.blocks().len();
        let label = if let Some(index) = self.navigation.current(&self.alignment) {
            format!("Change {} of {count}", index + 1)
        } else if count == 0 {
            "No changes".to_owned()
        } else if count == 1 {
            "1 change".to_owned()
        } else {
            format!("{count} changes")
        };

        let restore = self.selection_restore();
        let label = restore
            .as_ref()
            .map_or(label, |plan| self.restore_description(plan));

        div()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .w_full()
            .h(px(TOOLBAR_HEIGHT))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .overflow_hidden()
            .cursor_default()
            .font_family(cx.theme().font_family.clone())
            .text_size(px(13.0))
            .line_height(px(20.0))
            .bg(cx.theme().secondary)
            .border_b_1()
            .border_color(cx.theme().border)
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
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .children(restore.map(|plan| {
                Button::new("restore-selected-lines")
                    .icon(IconName::ArrowRight)
                    .label("Restore lines")
                    .ghost()
                    .with_size(px(28.0))
                    .tooltip("Restore outlined lines from baseline (Alt+Enter; undo: Ctrl+Z)")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.apply_selection_restore(&plan, window, cx);
                    }))
            }))
            .child(self.render_input_mode(cx))
    }

    fn render_input_mode(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = Self::vim_enabled(cx);
        let label = if enabled {
            self.vim.mode().label()
        } else {
            "Two-way comparison"
        };

        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .flex_shrink_0()
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(
                Switch::new("vim-mode")
                    .label("Vim mode")
                    .checked(enabled)
                    .small()
                    .on_change(cx.listener(|this, enabled, window, cx| {
                        this.toggle_vim(*enabled, window, cx);
                    })),
            )
    }

    pub(super) fn render_pane_header(
        &self,
        side: Side,
        pane_width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (name, directory) = path_labels(&self.document(side).path);
        let role = match side {
            Side::Left => "Baseline · Read-only",
            Side::Right => "Local · Memory only",
        };

        div()
            .absolute()
            .top(px(TOOLBAR_HEIGHT))
            .left(px(if side == Side::Left { 0.0 } else { pane_width }))
            .w(px(pane_width))
            .h(px(FILE_HEADER_HEIGHT))
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
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(12.0))
                            .text_color(cx.theme().muted_foreground)
                            .child(role),
                    ),
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
    use crate::editor::{GUTTER_WIDTH, HEADER_HEIGHT, LINE_HEIGHT, TEXT_INSET};
    use std::path::Path;
    use yori::geometry::EditorGeometry;

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
