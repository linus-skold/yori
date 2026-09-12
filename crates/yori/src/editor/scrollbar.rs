//! One shared diff-aware scrollbar beside the aligned rows.

use std::ops::Range;

use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    Bounds, Context, DispatchPhase, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Styled, TestSupportExt,
    WeakEntity, Window, canvas, div, fill, point, px, size,
};
use yori::scrollbar::{OverviewBand, ScrollTrack};

use super::{AlignedEditor, HEADER_HEIGHT, LINE_HEIGHT};
use crate::appearance;

pub(super) const WIDTH: f32 = 18.0;

impl AlignedEditor {
    fn scroll_track(&self) -> ScrollTrack {
        ScrollTrack::new(
            self.alignment.rows().len(),
            LINE_HEIGHT,
            self.geometry().rows_viewport_height(),
        )
    }

    fn scrollbar_y(&self, y: gpui_kit::Pixels) -> f32 {
        f32::from(y - self.content_bounds.get().origin.y) - HEADER_HEIGHT
    }

    fn scrollbar_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let track = self.scroll_track();
        let y = self.scrollbar_y(event.position.y);
        let thumb = track.thumb(self.vertical_scroll);
        let grab = if thumb.contains(&y) {
            y - thumb.start
        } else {
            self.vertical_scroll = track.jump(y);
            let thumb = track.thumb(self.vertical_scroll);
            (y - thumb.start).clamp(0.0, thumb.end - thumb.start)
        };

        self.scrollbar_grab = Some(grab);

        cx.stop_propagation();
        cx.notify();
    }

    fn scrollbar_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(grab) = self.scrollbar_grab else {
            return;
        };

        if event.pressed_button == Some(MouseButton::Left) {
            let track = self.scroll_track();
            let thumb = track.thumb(self.vertical_scroll);
            let top = self.scrollbar_y(event.position.y) - grab.min(thumb.end - thumb.start);
            self.vertical_scroll = track.scroll_for_thumb(top);
        } else {
            self.scrollbar_grab = None;
        }

        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn render_scrollbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let track = self.scroll_track();
        let thumb = track.thumb(self.vertical_scroll);
        let bands = if self.merge.is_none() {
            track.bands(&self.alignment, LINE_HEIGHT)
        } else {
            Vec::new()
        };
        let merge_bands: Vec<_> = self
            .merge
            .iter()
            .flat_map(|merge| {
                merge.session.conflicts().iter().map(|conflict| {
                    let color = if merge
                        .session
                        .state(conflict.id)
                        .expect("known conflict")
                        .resolved
                    {
                        cx.theme().muted_foreground
                    } else {
                        cx.theme().warning
                    };
                    (
                        track.marker(merge.conflicts[conflict.id.0].clone(), LINE_HEIGHT),
                        color,
                    )
                })
            })
            .collect();
        let current = if let Some(merge) = &self.merge {
            merge
                .current
                .map(|id| track.marker(merge.conflicts[id.0].clone(), LINE_HEIGHT))
        } else {
            self.navigation
                .current(&self.alignment)
                .map(|index| track.marker(self.alignment.blocks()[index].rows.clone(), LINE_HEIGHT))
        };
        let foreground = cx.theme().foreground;
        let thumb_color = foreground.opacity(if self.scrollbar_grab.is_some() {
            0.24
        } else {
            0.12
        });
        let editor = cx.weak_entity();

        div()
            .id("diff-scrollbar")
            .test_support()
            .absolute()
            .right_0()
            .top(px(HEADER_HEIGHT))
            .w(px(WIDTH))
            .h(px(self.geometry().rows_viewport_height()))
            .overflow_hidden()
            .cursor_default()
            .bg(cx.theme().secondary)
            .border_l_1()
            .border_color(cx.theme().border)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::scrollbar_down))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        paint_rect(bounds, 1.0..WIDTH - 1.0, thumb.clone(), thumb_color, window);
                        paint_markers(bounds, &bands, current.clone(), foreground, window);
                        for (rows, color) in &merge_bands {
                            paint_rect(bounds, 6.0..14.0, rows.clone(), *color, window);
                        }

                        capture_drag(editor.clone(), window);
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

fn paint_rect(
    bounds: Bounds<Pixels>,
    x: Range<f32>,
    y: Range<f32>,
    color: Hsla,
    window: &mut Window,
) {
    let rect = Bounds::new(
        bounds.origin + point(px(x.start), px(y.start)),
        size(px(x.end - x.start), px(y.end - y.start)),
    );

    window.paint_quad(fill(rect, color));
}

fn paint_markers(
    bounds: Bounds<Pixels>,
    bands: &[OverviewBand],
    current: Option<Range<f32>>,
    foreground: Hsla,
    window: &mut Window,
) {
    for band in bands {
        if band.left {
            paint_rect(
                bounds,
                4.0..8.0,
                band.top..band.bottom,
                appearance::removed().marker,
                window,
            );
        }
        if band.right {
            paint_rect(
                bounds,
                10.0..14.0,
                band.top..band.bottom,
                appearance::added().marker,
                window,
            );
        }
    }

    if let Some(current) = current {
        paint_rect(bounds, 1.0..3.0, current, foreground.opacity(0.85), window);
    }
}

fn capture_drag(editor: WeakEntity<AlignedEditor>, window: &mut Window) {
    // Capture window-wide movement so dragging outside the rail never turns
    // into text selection. These handlers exist only for this frame.
    let dragging_editor = editor.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
        if phase == DispatchPhase::Capture {
            let _ = dragging_editor.update(cx, |editor, cx| {
                editor.scrollbar_move(event, cx);
            });
        }
    });

    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
            let _ = editor.update(cx, |editor, cx| {
                if editor.scrollbar_grab.take().is_some() {
                    cx.stop_propagation();
                    cx.notify();
                }
            });
        }
    });
}

#[cfg(test)]
mod tests;
