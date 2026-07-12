//! Span and location types for source tracking.
//!
//! These types are stable and hand-written (not generated).

/// Byte offset span into the source buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[inline]
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: start as u32,
            end: end as u32,
        }
    }

    #[inline]
    pub fn empty() -> Self {
        Self { start: 0, end: 0 }
    }

    #[inline]
    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Source location for error reporting.
#[derive(Debug, Clone, Copy, Default)]
pub struct Location {
    pub line: u32,
    pub column: u32,
    pub byte_offset: u32,
}

impl Location {
    #[inline]
    pub fn new(line: u32, column: u32, byte_offset: usize) -> Self {
        Self {
            line,
            column,
            byte_offset: byte_offset as u32,
        }
    }
}
