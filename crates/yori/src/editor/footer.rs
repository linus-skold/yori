//! Quiet document controls beneath each pane, using the existing component kit.

use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
    tooltip::Tooltip,
};
use gpui_kit::{
    Anchor, Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, TestSupportExt, Window, div, px,
};

use super::{AlignedEditor, FOOTER_HEIGHT, Language, PaneDocument, Side};

impl AlignedEditor {
    pub(super) fn render_footer(
        &self,
        pane_width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("editor-footer")
            .test_support()
            .absolute()
            .bottom_0()
            .left_0()
            .w_full()
            .h(px(FOOTER_HEIGHT))
            .flex()
            .items_center()
            .overflow_hidden()
            .cursor_default()
            .font_family(cx.theme().font_family.clone())
            .text_size(px(12.0))
            .line_height(px(20.0))
            .bg(cx.theme().secondary)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(self.render_pane_status(Side::Left, pane_width, cx))
            .child(self.render_pane_status(Side::Right, pane_width, cx))
    }

    fn render_pane_status(
        &self,
        side: Side,
        width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pane = self.document(side);
        let tooltip = line_ending_description(pane);
        let show_metadata = width >= 360.0;

        div()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .overflow_hidden()
            .border_l(px(if side == Side::Right { 1.0 } else { 0.0 }))
            .border_color(cx.theme().border)
            .child(self.render_language_menu(side, cx))
            .children(show_metadata.then(|| {
                div()
                    .id(if side == Side::Left {
                        "left-line-endings"
                    } else {
                        "right-line-endings"
                    })
                    .text_color(cx.theme().muted_foreground)
                    .child(pane.line_endings.label())
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            }))
            .child(div().flex_1())
            .children((side == Side::Right).then(|| self.render_input_mode(cx)))
    }

    fn render_language_menu(&self, side: Side, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = self.document(side);
        let detected = Language::detect(&pane.path);
        let selected = pane.language_override;
        let label = pane.language().label();
        let tooltip = if selected.is_none() {
            format!("Language: Auto ({label}). Click to override.")
        } else {
            format!("Language: {label} (manual override)")
        };
        let editor = cx.weak_entity();

        Button::new(if side == Side::Left {
            "left-language"
        } else {
            "right-language"
        })
        .label(label)
        .dropdown_caret(true)
        .ghost()
        .with_size(px(24.0))
        .accessibility_label(if side == Side::Left {
            "Baseline language"
        } else {
            "Local language"
        })
        .tooltip(tooltip)
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, _, _| {
            let choices = std::iter::once(None).chain(Language::ALL.into_iter().map(Some));

            for language in choices {
                let label = language.map_or_else(
                    || format!("Auto ({})", detected.label()),
                    |language| language.label().to_owned(),
                );
                let editor = editor.clone();
                let item = PopupMenuItem::new(label)
                    .checked(language == selected)
                    .on_click(move |_, window, cx| {
                        let _ = editor.update(cx, |editor, cx| {
                            editor.choose_language(side, language, window, cx);
                        });
                    });
                menu = menu.item(item);
            }

            menu
        })
    }

    fn render_input_mode(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = Self::vim_enabled(cx);

        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .flex_shrink_0()
            .children(enabled.then(|| {
                div()
                    .w(px(80.0))
                    .text_size(px(11.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(self.vim.mode().label())
            }))
            .child(
                Switch::new("vim-mode")
                    .label("Vim")
                    .checked(enabled)
                    .small()
                    .on_change(cx.listener(|this, enabled, window, cx| {
                        this.toggle_vim(*enabled, window, cx);
                    })),
            )
    }

    fn choose_language(
        &mut self,
        side: Side,
        language: Option<Language>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        };
        pane.set_language(language);

        self.focus.focus(window, cx);
        cx.notify();
    }
}

fn line_ending_description(pane: &PaneDocument) -> String {
    let ending = pane.line_endings.description();
    if pane.document.text().is_empty() {
        return format!("{ending} · Empty file");
    }

    let final_newline = if pane.document.text().ends_with('\n') {
        "Final newline present"
    } else {
        "No final newline"
    };

    format!("{ending} · {final_newline}")
}

#[cfg(test)]
mod tests;
