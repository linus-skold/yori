//! Language-independent word/token emphasis. This does not change line pairing.

use similar::{Algorithm, DiffTag, TextDiff};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

// Keep pathological single lines from monopolizing the UI thread. The ordinary
// line diff remains authoritative when fine-grained emphasis takes this fallback.
const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_TOKENS: usize = 512;

/// Changed ranges in each document's original UTF-8 byte coordinates.
/// Only paired modified rows receive intraline emphasis; gaps remain line-only.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IntralineDiff {
    pub left: Vec<Range<usize>>,
    pub right: Vec<Range<usize>>,
}

impl IntralineDiff {
    #[must_use]
    pub fn between(left: &str, right: &str) -> Self {
        if left == right {
            return Self::default();
        }
        if left.len().max(right.len()) > MAX_LINE_BYTES {
            return Self::whole_lines(left, right);
        }
        let (Some(old), Some(new)) = (tokens(left), tokens(right)) else {
            return Self::whole_lines(left, right);
        };

        let old_text: Vec<_> = old.iter().map(|range| &left[range.clone()]).collect();
        let new_text: Vec<_> = new.iter().map(|range| &right[range.clone()]).collect();
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Myers)
            .diff_slices(&old_text, &new_text);

        let mut result = Self::default();
        for op in diff.ops() {
            if op.tag() != DiffTag::Equal {
                push_range(&mut result.left, &old, op.old_range());
                push_range(&mut result.right, &new, op.new_range());
            }
        }

        result
    }

    fn whole_lines(left: &str, right: &str) -> Self {
        let nonempty = |text: &str| {
            if text.is_empty() {
                Vec::new()
            } else {
                std::iter::once(0..text.len()).collect()
            }
        };

        Self {
            left: nonempty(left),
            right: nonempty(right),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Word,
    Space,
    Punctuation,
}

fn tokens(text: &str) -> Option<Vec<Range<usize>>> {
    let mut result: Vec<Range<usize>> = Vec::new();
    let mut previous = TokenKind::Punctuation;

    for (start, grapheme) in text.grapheme_indices(true) {
        let kind = if grapheme.chars().all(char::is_whitespace) {
            TokenKind::Space
        } else if grapheme.chars().any(|ch| ch.is_alphanumeric() || ch == '_') {
            TokenKind::Word
        } else {
            TokenKind::Punctuation
        };
        let end = start + grapheme.len();

        if kind != TokenKind::Punctuation && kind == previous {
            if let Some(last) = result.last_mut() {
                last.end = end;
            }
        } else {
            if result.len() == MAX_TOKENS {
                return None;
            }

            result.push(start..end);
        }
        previous = kind;
    }

    Some(result)
}

fn push_range(output: &mut Vec<Range<usize>>, tokens: &[Range<usize>], changed: Range<usize>) {
    if !changed.is_empty() {
        let range = tokens[changed.start].start..tokens[changed.end - 1].end;
        if let Some(last) = output.last_mut().filter(|last| last.end == range.start) {
            last.end = range.end;
        } else {
            output.push(range);
        }
    }
}
