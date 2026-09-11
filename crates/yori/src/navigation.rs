//! Current-change navigation, separate from source selection and scroll position.

use std::ops::Range;
use yori_diff::Alignment;

#[derive(Debug, Clone, Copy)]
pub enum ChangeDirection {
    Previous,
    Next,
}

#[derive(Debug, Default)]
enum Anchor {
    #[default]
    Start,
    Row(usize),
    Change(usize),
}

#[derive(Debug, Default)]
pub struct ChangeNavigation {
    anchor: Anchor,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChangeTarget {
    pub index: usize,
    pub rows: Range<usize>,
    pub right_offset: usize,
}

impl ChangeNavigation {
    /// Follow a manually positioned caret/selection. Call again after realignment
    /// so no change index from an older comparison is retained.
    pub fn locate(&mut self, row: usize) {
        self.anchor = Anchor::Row(row);
    }

    #[must_use]
    pub fn current(&self, alignment: &Alignment) -> Option<usize> {
        let blocks = alignment.blocks();
        match self.anchor {
            Anchor::Start => None,
            Anchor::Change(index) => blocks.get(index).map(|_| index),
            Anchor::Row(row) => {
                let index = blocks.partition_point(|block| block.rows.end <= row);
                blocks
                    .get(index)
                    .filter(|block| block.rows.contains(&row))
                    .map(|_| index)
            }
        }
    }

    /// Stop at either end rather than silently wrapping around the comparison.
    #[must_use]
    pub fn target(&self, alignment: &Alignment, direction: ChangeDirection) -> Option<usize> {
        let blocks = alignment.blocks();
        let index = if let Some(current) = self.current(alignment) {
            match direction {
                ChangeDirection::Previous => current.checked_sub(1)?,
                ChangeDirection::Next => current + 1,
            }
        } else {
            match (&self.anchor, direction) {
                (Anchor::Row(row), ChangeDirection::Previous) => blocks
                    .partition_point(|block| block.rows.end <= *row)
                    .checked_sub(1)?,
                (Anchor::Row(row), ChangeDirection::Next) => {
                    blocks.partition_point(|block| block.rows.start < *row)
                }
                (_, ChangeDirection::Previous) => return None,
                (_, ChangeDirection::Next) => 0,
            }
        };

        blocks.get(index).map(|_| index)
    }

    pub fn advance(
        &mut self,
        alignment: &Alignment,
        direction: ChangeDirection,
    ) -> Option<ChangeTarget> {
        let index = self.target(alignment, direction)?;
        let block = &alignment.blocks()[index];

        // Keep the change itself selected even when a deletion's right-hand
        // insertion position maps to an unchanged row or the EOF canvas.
        self.anchor = Anchor::Change(index);

        Some(ChangeTarget {
            index,
            rows: block.rows.clone(),
            right_offset: block.right.start,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChangeDirection::{Next, Previous},
        ChangeNavigation,
    };
    use yori_diff::{Alignment, restore_block};
    use yori_document::{
        Document,
        editing::{EditHistory, TextSelection},
    };

    fn doc(text: &str) -> Document {
        Document::from_bytes(text.as_bytes().to_vec()).unwrap()
    }

    #[test]
    fn visits_each_block_once_and_stops_at_both_ends() {
        let left = doc("old\nkeep\ndeleted\nend\n");
        let right = doc("new\nkeep\nend\nadded\n");
        let alignment = Alignment::between(&left, &right);
        assert_eq!(alignment.blocks().len(), 3);

        let mut nav = ChangeNavigation::default();
        assert_eq!(nav.current(&alignment), None);
        assert_eq!(nav.target(&alignment, Previous), None);

        for index in 0..3 {
            let target = nav.advance(&alignment, Next).unwrap();

            assert_eq!(target.index, index);
            assert_eq!(target.right_offset, alignment.blocks()[index].right.start);
            assert_eq!(target.rows, alignment.blocks()[index].rows);
            assert_eq!(nav.current(&alignment), Some(index));
        }
        assert!(nav.advance(&alignment, Next).is_none());

        assert_eq!(nav.advance(&alignment, Previous).unwrap().index, 1);
        assert_eq!(nav.advance(&alignment, Previous).unwrap().index, 0);
        assert!(nav.advance(&alignment, Previous).is_none());
        assert_eq!(nav.current(&alignment), Some(0));
    }

    #[test]
    fn manual_positions_inside_and_between_blocks_anchor_navigation() {
        let left = doc("same\nold\nbetween\nold again\nend\n");
        let right = doc("same\nnew\nbetween\nnew again\nend\n");
        let alignment = Alignment::between(&left, &right);
        let mut nav = ChangeNavigation::default();

        nav.locate(0);
        assert_eq!(nav.target(&alignment, Next), Some(0));

        nav.locate(1);
        assert_eq!(nav.current(&alignment), Some(0));
        assert_eq!(nav.target(&alignment, Next), Some(1));

        nav.locate(2);
        assert_eq!(nav.current(&alignment), None);
        assert_eq!(nav.target(&alignment, Previous), Some(0));
        assert_eq!(nav.target(&alignment, Next), Some(1));

        nav.locate(5);
        assert_eq!(nav.target(&alignment, Previous), Some(1));
        assert_eq!(nav.target(&alignment, Next), None);
    }

    #[test]
    fn deletions_keep_their_identity_at_insertion_boundaries_and_empty_sides() {
        for (left, right) in [
            ("deleted\nkeep\n", "keep\n"),
            ("keep\ndeleted", "keep\n"),
            ("deleted\n", ""),
            ("", "added\n"),
        ] {
            let (left, right) = (doc(left), doc(right));
            let alignment = Alignment::between(&left, &right);
            let mut nav = ChangeNavigation::default();

            let target = nav.advance(&alignment, Next).unwrap();

            assert_eq!(target.right_offset, alignment.blocks()[0].right.start);
            assert!(right.text().is_char_boundary(target.right_offset));
            assert_eq!(nav.current(&alignment), Some(0));
            assert!(nav.advance(&alignment, Next).is_none());
            assert!(nav.advance(&alignment, Previous).is_none());
        }
    }

    #[test]
    fn realignment_discards_old_indices_and_equal_files_have_no_targets() {
        let left = doc("first\nkeep\nsecond\n");
        let mut right = doc("changed\nkeep\ndifferent\n");
        let alignment = Alignment::between(&left, &right);
        let mut nav = ChangeNavigation::default();

        nav.advance(&alignment, Next).unwrap();
        nav.advance(&alignment, Next).unwrap();

        let mut history = EditHistory::default();
        let edit = restore_block(
            &mut history,
            &left,
            &mut right,
            TextSelection::caret(0),
            &alignment.blocks()[1],
        )
        .unwrap();

        let alignment = Alignment::between(&left, &right);
        nav.locate(alignment.row_for_offset(&right, edit.selection.head, false));

        assert_eq!(nav.current(&alignment), None);
        assert_eq!(nav.target(&alignment, Previous), Some(0));
        assert_eq!(nav.target(&alignment, Next), None);

        let equal = Alignment::between(&left, &left);
        nav.locate(0);
        assert!(nav.advance(&equal, Next).is_none());
        assert!(nav.advance(&equal, Previous).is_none());
    }
}
