//! A single window of independent comparison editors, using the existing component kit.

mod tabs;
#[cfg(test)]
mod tests;

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Root, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
    notification::Notification,
    tab::{Tab, TabBar, TabVariant},
    tooltip::Tooltip,
};
use gpui_kit::{
    App, AppContext, AsyncWindowContext, Context, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyBinding, MouseButton, ParentElement, PathPromptOptions,
    Render, ScrollHandle, StatefulInteractiveElement, Styled, Subscription, TestSupportExt, Window,
    div, px,
};
use yori_document::Document;

use crate::editor::{AlignedEditor, DirtyChanged, PaneDocument};
use tabs::{FilePair, Tabs};

const KEY_CONTEXT: &str = "ComparisonWorkspace";

gpui_kit::actions!(
    workspace,
    [OpenComparison, CloseComparison, Quit, NextTab, PreviousTab]
);

struct OpenTab {
    editor: Entity<AlignedEditor>,
    _subscription: Subscription,
}

pub(super) struct Workspace {
    tabs: Tabs<OpenTab>,
    focus: FocusHandle,
    tab_scroll: ScrollHandle,
    picking_files: bool,
}

impl Workspace {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                if !this.has_modified_tabs(cx) {
                    return true;
                }

                this.request_close(None, window, cx);
                false
            })
            .unwrap_or(true)
        });

        let focus = cx.focus_handle();
        focus.focus(window, cx);

        Self {
            tabs: Tabs::default(),
            focus,
            tab_scroll: ScrollHandle::new(),
            picking_files: false,
        }
    }

    pub(super) fn open_paths(
        &mut self,
        left: &std::path::Path,
        right: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result =
            FilePair::resolve(left, right).and_then(|pair| self.open_pair(pair, window, cx));
        if let Err(error) = result {
            window.push_notification(Notification::error(error), cx);
        }
    }

    /// Process one CLI handoff on the UI thread. Completion means every pair was
    /// loaded or rejected, not just queued; temporary files can then be released.
    pub(super) fn open_comparisons(
        &mut self,
        pairs: &[(std::path::PathBuf, std::path::PathBuf)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        window.activate_window();
        if pairs.is_empty() {
            if !self.picking_files && !window.has_active_dialog(cx) {
                self.focus_active(window, cx);
            }
            return Ok(());
        }
        if self.picking_files || window.has_active_dialog(cx) {
            return Err("yori has a dialog open; finish or cancel it, then retry".into());
        }

        let mut errors = Vec::new();
        for (left, right) in pairs {
            let result =
                FilePair::resolve(left, right).and_then(|pair| self.open_pair(pair, window, cx));
            if let Err(error) = result {
                window.push_notification(Notification::error(error.clone()), cx);
                errors.push(error);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    fn open_pair(
        &mut self,
        pair: FilePair,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(id) = self.tabs.find(&pair) {
            self.activate(id, window, cx);
            return Ok(());
        }

        let left = PaneDocument::new(pair.left.clone(), Document::read(&pair.left)?);
        let right = PaneDocument::new(pair.right.clone(), Document::read(&pair.right)?);
        self.deactivate(cx);
        let editor = cx.new(|cx| AlignedEditor::new(left, right, window, cx));
        let subscription = cx.subscribe(&editor, |_, _, _: &DirtyChanged, cx| cx.notify());
        self.tabs.insert(
            pair,
            OpenTab {
                editor,
                _subscription: subscription,
            },
        );

        cx.notify();
        Ok(())
    }

    fn deactivate(&self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.active.and_then(|id| self.tabs.get(id)) {
            tab.content.editor.update(cx, AlignedEditor::deactivate);
        }
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.active.and_then(|id| self.tabs.get(id)) {
            tab.content.editor.focus_handle(cx).focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
    }

    fn activate(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.deactivate(cx);
        self.tabs.activate(id);

        self.focus_active(window, cx);
        cx.notify();
    }

    fn cycle(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self
            .tabs
            .entries
            .iter()
            .position(|tab| Some(tab.id) == self.tabs.active)
        else {
            return;
        };

        let count = self.tabs.entries.len();
        let next = if backwards {
            (index + count - 1) % count
        } else {
            (index + 1) % count
        };
        self.activate(self.tabs.entries[next].id, window, cx);
    }

    fn has_modified_tabs(&self, cx: &App) -> bool {
        self.tabs
            .requires_discard_confirmation(None, |tab| tab.editor.read(cx).is_dirty())
    }

    fn close(&mut self, target: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = target else {
            window.defer(cx, |window, _| window.remove_window());
            return;
        };

        if self.tabs.active == Some(id) {
            self.deactivate(cx);
        }
        self.tabs.remove(id);

        self.focus_active(window, cx);
        cx.notify();
    }

    fn request_close(
        &mut self,
        target: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }

        let modified = self
            .tabs
            .requires_discard_confirmation(target, |tab| tab.editor.read(cx).is_dirty());
        if !modified {
            self.close(target, window, cx);
            return;
        }

        self.deactivate(cx);
        self.focus_active(window, cx);

        let title = target.map_or_else(
            || "Discard edits and close yori?".to_owned(),
            |id| format!("Discard edits to {}?", self.tabs.label(id)),
        );
        let view = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let footer = DialogFooter::new()
                .child(
                    Button::new("cancel")
                        .label("Keep open")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(Cancel), cx);
                        }),
                )
                .child(
                    Button::new("ok")
                        .label("Discard edits")
                        .primary()
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
                        }),
                );

            dialog
                .title(title.clone())
                .child("Edits are only held in memory. Closing will discard them; saving is not available yet.")
                .overlay_closable(false)
                .footer(footer)
                .on_ok(move |_, window, cx| {
                    // Restore modal focus before disposing the editor it belonged to.
                    window.close_dialog(cx);
                    let _ = view.update(cx, |this, cx| this.close(target, window, cx));
                    false
                })
        });
    }

    fn choose_pair(&mut self, _: &OpenComparison, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking_files || window.has_active_dialog(cx) {
            return;
        }

        self.deactivate(cx);
        self.picking_files = true;
        cx.notify();

        cx.spawn_in(window, async move |view, cx| {
            let result = choose_pair(cx).await;
            let _ = cx.update(|window, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.picking_files = false;
                    match result {
                        Ok(Some((left, right))) => this.open_paths(&left, &right, window, cx),
                        Ok(None) => this.focus_active(window, cx),
                        Err(error) => window.push_notification(Notification::error(error), cx),
                    }

                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.0))
            .child("Compare two files")
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Choose a baseline, then a local file. Nothing is written to disk."),
            )
            .child(
                Button::new("open-first-comparison")
                    .label("Open comparison")
                    .icon(IconName::Plus)
                    .ghost()
                    .disabled(self.picking_files)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.choose_pair(&OpenComparison, window, cx);
                    })),
            )
    }

    fn render_tab(&self, tab: &tabs::Tab<OpenTab>, cx: &mut Context<Self>) -> Tab {
        let id = tab.id;
        let label = self.tabs.label(id);
        let description = tab.pair.description();
        let modified = tab.content.editor.read(cx).is_dirty();
        let accessible = format!(
            "{label}{}; {description}",
            if modified { "; modified" } else { "" }
        );

        Tab::new()
            .label(label)
            .aria_label(accessible)
            .prefix(
                div()
                    .id(("comparison-paths", id))
                    .pl(px(10.0))
                    .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
                    .child(Icon::new(IconName::FileText).with_size(px(14.0))),
            )
            .suffix(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().size(px(6.0)).rounded_full().bg(if modified {
                        cx.theme().foreground
                    } else {
                        cx.theme().transparent
                    }))
                    .child(
                        div()
                            .id(("tab-close-target", id))
                            .test_support()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                Button::new(("close-comparison", id))
                                    .icon(IconName::Close)
                                    .ghost()
                                    .with_size(px(22.0))
                                    .accessibility_label("Close comparison")
                                    .tooltip("Close comparison (Ctrl+W)")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.request_close(Some(id), window, cx);
                                    })),
                            ),
                    ),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self
            .tabs
            .entries
            .iter()
            .position(|tab| Some(tab.id) == self.tabs.active);
        let tabs = self
            .tabs
            .entries
            .iter()
            .map(|tab| self.render_tab(tab, cx))
            .collect::<Vec<_>>();
        let ids = self
            .tabs
            .entries
            .iter()
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        let mut bar = TabBar::new("comparisons")
            .with_variant(TabVariant::Tab)
            .with_size(px(38.0))
            .track_scroll(&self.tab_scroll)
            .max_width(px(260.0))
            .menu(true)
            .children(tabs)
            .on_click(cx.listener(move |this, index: &usize, window, cx| {
                if let Some(id) = ids.get(*index) {
                    this.activate(*id, window, cx);
                }
            }));
        if let Some(index) = selected {
            bar = bar.selected_index(index);
        }

        let body = if let Some(tab) = self.tabs.active.and_then(|id| self.tabs.get(id)) {
            div().size_full().child(tab.content.editor.clone())
        } else {
            div().size_full().child(self.render_empty(cx))
        };

        let dialogs = Root::render_dialog_layer(window, cx);
        let notifications = Root::render_notification_layer(window, cx);

        div()
            .id("workspace")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(cx.theme().font_family.clone())
            .text_size(px(13.0))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::choose_pair))
            .on_action(cx.listener(|this, _: &CloseComparison, window, cx| {
                if let Some(id) = this.tabs.active {
                    this.request_close(Some(id), window, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &Quit, window, cx| this.request_close(None, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle(false, window, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousTab, window, cx| this.cycle(true, window, cx)),
            )
            .child(
                div()
                    .h(px(38.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .bg(cx.theme().tab_bar)
                    .child(div().flex_1().min_w_0().overflow_hidden().child(bar))
                    .child(
                        Button::new("open-comparison")
                            .icon(IconName::Plus)
                            .ghost()
                            .with_size(px(28.0))
                            .accessibility_label("Open comparison")
                            .tooltip("Open comparison (Ctrl+O)")
                            .disabled(self.picking_files)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_pair(&OpenComparison, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(body),
            )
            .children(dialogs)
            .children(notifications)
    }
}

async fn choose_file(
    cx: &mut AsyncWindowContext,
    prompt: &'static str,
) -> Result<Option<std::path::PathBuf>, String> {
    let request = cx
        .update(|_, cx| {
            cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(prompt.into()),
            })
        })
        .map_err(|error| error.to_string())?;
    let result = request
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;

    Ok(result.and_then(|paths| paths.into_iter().next()))
}

async fn choose_pair(
    cx: &mut AsyncWindowContext,
) -> Result<Option<(std::path::PathBuf, std::path::PathBuf)>, String> {
    let Some(left) = choose_file(cx, "Select baseline file").await? else {
        return Ok(None);
    };
    let Some(right) = choose_file(cx, "Select local file").await? else {
        return Ok(None);
    };

    Ok(Some((left, right)))
}

pub(super) fn init(cx: &mut App) {
    let command = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };

    cx.bind_keys([
        KeyBinding::new(&format!("{command}-o"), OpenComparison, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-w"), CloseComparison, Some(KEY_CONTEXT)),
        KeyBinding::new(&format!("{command}-q"), Quit, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-tab", NextTab, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, Some(KEY_CONTEXT)),
    ]);
}
