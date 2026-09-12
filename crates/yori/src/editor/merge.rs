//! Three-way merging on the shared native editor. The session owns
//! canonical result text/history; `PaneDocument` is its synchronized rendering cache.

mod chrome;
mod controls;
mod layout;
mod selection;
#[cfg(test)]
mod tests;

use std::ops::Range;

use gpui_kit::{ClipboardItem, Context, Window};
use yori::geometry::display_units;
use yori_diff::{
    Alignment,
    merge::{ConflictId, MergeError, MergeInput, MergeRow, MergeSession, MergeUpdate, Take},
};
#[cfg(test)]
use yori_document::Document;
use yori_document::editing::TextSelection;

use super::{AlignedEditor, DirtyChanged, LINE_HEIGHT, PaneDocument, Selection, Side};

pub(super) struct MergeState {
    pub session: MergeSession,
    pub incoming: PaneDocument,
    pub incoming_alignment: Alignment,
    pub revision: u64,
    pub hovered_lines: Option<MergeInput>,
    pub base_columns: usize,
    pub rows: Vec<MergeRow>,
    pub conflicts: Vec<Range<usize>>,
    pub headers: Vec<usize>,
    pub hovered: Option<(ConflictId, Take)>,
    pub current: Option<ConflictId>,
    pub show_base: bool,
    pub base_preview: Option<(Range<usize>, Range<usize>)>,
}

impl AlignedEditor {
    #[cfg(test)]
    pub(crate) fn merge_fixture(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let source = |text: &str| {
            Document::from_bytes(text.as_bytes().to_vec()).expect("valid merge fixture")
        };
        let session = MergeSession::new(
            source(include_str!("../../fixtures/merge/base.rs")),
            source(include_str!("../../fixtures/merge/local.rs")),
            source(include_str!("../../fixtures/merge/incoming.rs")),
        )
        .expect("valid merge fixture");

        Self::from_merge_session(session, window, cx)
    }

    #[cfg(test)]
    pub(super) fn from_merge_session(
        session: MergeSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let paths = crate::comparison::MergePaths {
            base: "base.rs".into(),
            local: "local.rs".into(),
            incoming: "incoming.rs".into(),
            result: "result.rs".into(),
        };

        Self::new_merge(&paths, session, window, cx)
    }

    pub(crate) fn new_merge(
        paths: &crate::comparison::MergePaths,
        session: MergeSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let local = PaneDocument::new(paths.local.clone(), session.local().clone());
        let result = PaneDocument::new(paths.result.clone(), session.result().clone());
        let incoming = PaneDocument::new(paths.incoming.clone(), session.incoming().clone());
        let incoming_alignment = Alignment::between(session.incoming(), session.result());
        let base_columns = super::max_display_columns(session.base(), super::TAB_WIDTH);

        let current = session.conflicts().first().map(|conflict| conflict.id);
        let offset = current
            .and_then(|id| session.state(id))
            .map_or(0, |state| state.result.start);

        let mut editor = Self::new(local, result, window, cx);
        editor.merge = Some(MergeState {
            session,
            incoming,
            incoming_alignment,
            revision: 0,
            hovered_lines: None,
            base_columns,
            rows: Vec::new(),
            conflicts: Vec::new(),
            headers: Vec::new(),
            hovered: None,
            current,
            show_base: false,
            base_preview: None,
        });
        editor.selection = Some(Selection {
            side: Side::Right,
            anchor: offset,
            head: offset,
        });
        editor.refresh_merge_projection();

        editor
    }

    pub(super) fn refresh_merge_projection(&mut self) {
        let Some(merge) = &mut self.merge else {
            return;
        };

        merge.project_rows();
        merge.revision += 1;
        merge.hovered_lines = None;
        merge.incoming_alignment = Alignment::from_projection(
            &merge.incoming.document,
            &self.right.document,
            merge.rows.iter().map(|row| (row.incoming, row.result)),
        );

        self.alignment = Alignment::from_projection(
            &self.left.document,
            &self.right.document,
            merge.rows.iter().map(|row| (row.local, row.result)),
        );
    }

    pub(super) fn apply_merge_update(
        &mut self,
        update: MergeUpdate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = self.view_anchor();
        self.right.document = self
            .merge
            .as_ref()
            .expect("merge mode")
            .session
            .result()
            .clone();
        self.selection = Some(Selection {
            side: Side::Right,
            anchor: update.selection.anchor,
            head: update.selection.head,
        });

        if let Some(edit) = update.edit {
            self.finish_edit(&anchor, &edit, window, cx);
        } else {
            self.refresh_merge_projection();
            cx.notify();
        }

        cx.emit(DirtyChanged);
    }

    pub(super) fn merge_take(
        &mut self,
        id: ConflictId,
        take: Take,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_vim();
        self.finish_composition();
        let selection = self.right_selection().unwrap_or(TextSelection::caret(0));
        let merge = self.merge.as_mut().expect("merge mode");
        merge.current = Some(id);
        let update = merge.session.take(id, take, selection);

        self.finish_merge_action(id, update, window, cx);
    }

    pub(super) fn merge_reset(
        &mut self,
        id: ConflictId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_vim();
        self.finish_composition();
        let selection = self.right_selection().unwrap_or(TextSelection::caret(0));
        let merge = self.merge.as_mut().expect("merge mode");
        merge.current = Some(id);
        let update = merge.session.reset(id, selection);

        self.finish_merge_action(id, update, window, cx);
    }

    fn finish_merge_action(
        &mut self,
        id: ConflictId,
        update: Result<MergeUpdate, MergeError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match update {
            Ok(update) => self.apply_merge_update(update, window, cx),
            Err(error) => eprintln!("merge action rejected: {error}"),
        }

        // Taking a deletion can leave the caret in the next conflict. Keep the
        // action's explicit target, not that neighboring caret-derived target.
        if self.merge.as_ref().expect("merge mode").current != Some(id) {
            self.merge.as_mut().expect("merge mode").current = Some(id);
            self.refresh_merge_projection();
        }

        self.focus.focus(window, cx);
    }

    pub(super) fn merge_mark(
        &mut self,
        id: ConflictId,
        resolved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_vim();
        self.finish_composition();
        let selection = self.right_selection().unwrap_or(TextSelection::caret(0));
        let merge = self.merge.as_mut().expect("merge mode");
        merge.current = Some(id);
        let update = merge.session.set_resolved(id, resolved, selection);

        self.finish_merge_action(id, update, window, cx);
    }

    pub(super) fn merge_target(&self, previous: bool) -> Option<ConflictId> {
        let merge = self.merge.as_ref()?;

        if previous {
            merge
                .session
                .unresolved()
                .filter(|id| merge.current.is_some_and(|current| id.0 < current.0))
                .last()
        } else {
            merge
                .session
                .unresolved()
                .find(|id| merge.current.is_none_or(|current| id.0 > current.0))
        }
    }

    pub(super) fn navigate_merge(
        &mut self,
        previous: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.merge_target(previous) else {
            return;
        };

        self.cancel_vim();
        self.finish_composition();

        let merge = self.merge.as_mut().expect("merge mode");
        merge.current = Some(id);
        let offset = merge
            .session
            .state(id)
            .expect("known conflict")
            .result
            .start;
        self.selection = Some(Selection {
            side: Side::Right,
            anchor: offset,
            head: offset,
        });
        self.refresh_merge_projection();

        let row = self.merge.as_ref().expect("merge mode").headers[id.0];
        self.vertical_scroll = self
            .geometry()
            .change_scroll_top(row, self.alignment.rows().len());
        self.horizontal_scroll = 0.0;

        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn locate_merge_row(&mut self, row: usize) {
        let Some(merge) = &mut self.merge else {
            return;
        };

        let current = merge
            .conflicts
            .iter()
            .position(|rows| rows.contains(&row))
            .map(ConflictId);
        if let Some(current) = current
            && Some(current) != merge.current
        {
            merge.current = Some(current);
            if merge.show_base {
                self.refresh_merge_projection();
            }
        }
    }

    pub(super) fn toggle_merge_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition();
        let merge = self.merge.as_mut().expect("merge mode");
        merge.show_base = !merge.show_base;
        let current = merge.current;
        self.refresh_merge_projection();

        if let Some(id) = current {
            let merge = self.merge.as_ref().expect("merge mode");
            let top = merge
                .base_preview
                .as_ref()
                .map_or(merge.conflicts[id.0].start, |(rows, _)| rows.start);
            self.vertical_scroll = (display_units(top) * LINE_HEIGHT).min(
                self.geometry()
                    .vertical_scroll_limit(self.alignment.rows().len()),
            );
        }

        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn copy_merge_base(&self, id: ConflictId, cx: &mut Context<Self>) {
        if let Some(merge) = &self.merge {
            let text = merge
                .session
                .base()
                .copy_range(merge.session.conflicts()[id.0].base.clone());
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        }
    }
}
