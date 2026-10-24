//! Byte spans and line/column mapping.

use serde::{Deserialize, Serialize};

/// Half-open byte range `[start, end)` inside a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    pub fn contains(&self, off: usize) -> bool {
        off >= self.start && off < self.end
    }

    /// Slice of `src` covered by this span (empty string when out of bounds).
    pub fn slice<'a>(&self, src: &'a str) -> &'a str {
        src.get(self.start..self.end).unwrap_or("")
    }
}

/// Offsets of every line start, used to turn byte offsets into 1-based line/col.
#[derive(Debug, Clone, Default)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { starts }
    }

    /// 1-based (line, column). Column counts characters, not bytes.
    pub fn line_col(&self, src: &str, offset: usize) -> (u32, u32) {
        let offset = offset.min(src.len());
        let idx = match self.starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.starts.get(idx).copied().unwrap_or(0);
        let col = src
            .get(line_start..offset)
            .map(|s| s.chars().count() as u32)
            .unwrap_or(0);
        (idx as u32 + 1, col + 1)
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_basics() {
        let src = "abc\ndef\n";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_col(src, 0), (1, 1));
        assert_eq!(idx.line_col(src, 2), (1, 3));
        assert_eq!(idx.line_col(src, 4), (2, 1));
        assert_eq!(idx.line_col(src, 6), (2, 3));
    }

    #[test]
    fn line_col_counts_chars_not_bytes() {
        let src = "咖啡 4.5\n";
        let idx = LineIndex::new(src);
        // "咖" is 3 bytes, so byte 3 is the second char of line 1.
        assert_eq!(idx.line_col(src, 3), (1, 2));
    }
}
