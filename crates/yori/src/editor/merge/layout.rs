//! Presentation-only action rows around the session's source projection.

use super::{ConflictId, MergeRow, MergeState};

impl MergeState {
    pub(super) fn project_rows(&mut self) {
        let projection = self.session.alignment();
        let spans: Vec<_> = self
            .session
            .conflicts()
            .iter()
            .map(|conflict| {
                projection
                    .conflict_rows(conflict.id)
                    .expect("known conflict")
            })
            .collect();
        let mut starts: Vec<_> = spans
            .iter()
            .enumerate()
            .map(|(index, rows)| (rows.start, ConflictId(index)))
            .collect();
        starts.sort_unstable_by_key(|&(row, id)| (row, id.0));
        let mut starts = starts.into_iter().peekable();

        self.rows.clear();
        self.headers = vec![0; spans.len()];
        self.base_preview = None;
        self.hovered = None;
        let mut mapped = Vec::with_capacity(projection.rows().len() + 1);

        // Insert controls before source rows, then map both ends of each span.
        // No header or ancestor excerpt can acquire a source line or copyable byte.
        for boundary in 0..=projection.rows().len() {
            while let Some(&(start, id)) = starts.peek() {
                if start != boundary {
                    break;
                }

                starts.next();
                self.headers[id.0] = self.rows.len();
                self.push_space(1);
                if self.show_base && self.current == Some(id) {
                    self.insert_base_preview(id);
                }
            }

            mapped.push(self.rows.len());
            if let Some(row) = projection.rows().get(boundary) {
                self.rows.push(row.clone());
            }
        }

        self.conflicts = spans
            .iter()
            .map(|rows| {
                let end = if rows.is_empty() {
                    mapped[rows.start]
                } else {
                    mapped[rows.end - 1] + 1
                };
                mapped[rows.start]..end
            })
            .collect();
    }

    fn insert_base_preview(&mut self, id: ConflictId) {
        let bytes = &self.session.conflicts()[id.0].base;
        let base = self.session.base();
        let first = base
            .lines()
            .partition_point(|line| line.full.end <= bytes.start);
        let last = if bytes.is_empty() {
            first
        } else {
            base.line_at_offset(bytes.end - 1) + 1
        };
        let top = self.rows.len();
        let count = 1 + (last - first).max(1);

        self.push_space(count);
        self.base_preview = Some((top..top + count, first..last));
    }

    fn push_space(&mut self, count: usize) {
        self.rows.extend(std::iter::repeat_n(
            MergeRow {
                local: None,
                result: None,
                incoming: None,
            },
            count,
        ));
    }
}
