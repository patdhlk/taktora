//! Maps absolute byte offsets in the source XML to 1-based line/column spans.

use crate::error::Span;

/// Precomputed index of line-start byte offsets for O(log n) position lookup.
pub struct LineIndex {
    /// Byte offset of the start of each line (line 0 starts at 0).
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a line index over the source text.
    pub fn new(src: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    /// Convert a byte offset to a 1-based [`Span`]. Offsets past EOF clamp to
    /// the last line.
    pub fn span(&self, byte_offset: usize) -> Span {
        let line = match self.line_starts.binary_search(&byte_offset) {
            Ok(exact) => exact,
            Err(next) => next - 1,
        };
        let column = byte_offset - self.line_starts[line] + 1;
        Span {
            line: u32::try_from(line).unwrap_or(u32::MAX).saturating_add(1),
            column: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }
}
