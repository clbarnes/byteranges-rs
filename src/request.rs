use std::{fmt::Display, ops::RangeBounds};

pub const BYTES: &'static str = "bytes";
pub const RANGE: &'static str = "Range";

/// A single range in a `Range` request.
///
/// The [HttpRange::Range] variant can be created from rust ranges, like
///
/// ```rust
/// # use byteranges::request::HttpRange;
/// let range: HttpRange = (50..150).into();
/// ```
pub enum HttpRange {
    /// A range with a given start point and possibly an end point (otherwise EOF).
    Range { start: u64, end: Option<u64> },
    /// A range defined as the number of bytes at the end.
    Suffix(u64),
}

impl Display for HttpRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpRange::Range { start, end } => {
                f.write_fmt(format_args!("{start}-"))?;
                if let Some(e) = end {
                    f.write_fmt(format_args!("{e}"))?;
                }
                Ok(())
            }
            HttpRange::Suffix(len) => f.write_fmt(format_args!("-{len}")),
        }
    }
}

impl<T: RangeBounds<u64>> From<T> for HttpRange {
    fn from(value: T) -> Self {
        use std::ops::Bound::*;
        let start = match value.start_bound() {
            Included(i) => *i,
            Excluded(i) => i + &1,
            Unbounded => 0,
        };
        let end = match value.end_bound() {
            Included(i) => Some(*i),
            Excluded(i) => Some(i - &1),
            Unbounded => None,
        };
        HttpRange::Range {
            start: start.into(),
            end: end.into(),
        }
    }
}

/// Representation of a HTTP `Range` header.
///
/// By default, uses `bytes` units.
/// Implements [FromIterator] for anything which can be turned into a [HttpRange], e.g.
///
/// ```rust
/// # use byteranges::request::{HttpRange, RangeHeader};
/// let header: RangeHeader = [0..50, 125..150].into_iter().collect();
/// ```
pub struct RangeHeader<'a> {
    unit: &'a str,
    ranges: Vec<HttpRange>,
}

impl<'a> RangeHeader<'a> {
    /// Create a new header with the given units.
    pub fn new(unit: &'a str) -> Self {
        Self {
            unit,
            ranges: Vec::default(),
        }
    }

    /// Add a new range.
    pub fn push<R: Into<HttpRange>>(&mut self, range: R) -> &mut Self {
        self.ranges.push(range.into());
        self
    }

    /// Add a number of new ranges.
    pub fn extend<R: Into<HttpRange>, I: IntoIterator<Item = R>>(
        &mut self,
        ranges: I,
    ) -> &mut Self {
        for r in ranges {
            self.ranges.push(r.into());
        }
        self
    }
}

impl Default for RangeHeader<'_> {
    fn default() -> Self {
        Self {
            unit: BYTES,
            ranges: Vec::default(),
        }
    }
}

impl Display for RangeHeader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some(range_string) = self.ranges.iter().map(|r| r.to_string()).reduce(|accum, next| accum + ", " + &next) else {
            return Ok(());
        };
        f.write_fmt(format_args!("{0}={range_string}", self.unit))
    }
}

impl<R: Into<HttpRange>> FromIterator<R> for RangeHeader<'static> {
    fn from_iter<T: IntoIterator<Item = R>>(iter: T) -> Self {
        let mut h = RangeHeader::default();
        h.extend(iter);
        h
    }
}
