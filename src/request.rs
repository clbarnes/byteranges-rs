use std::{fmt::Display, ops::RangeBounds};

pub const BYTES: &str = "bytes";
pub const RANGE: &str = "Range";

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
            Excluded(i) => i + 1,
            Unbounded => 0,
        };
        let end = match value.end_bound() {
            Included(i) => Some(*i),
            Excluded(i) => Some(i - 1),
            Unbounded => None,
        };
        HttpRange::Range { start, end }
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

    pub fn to_header(&self, newline: bool) -> Vec<u8> {
        let s = self.to_string();
        let suffix = if newline { "\r\n" } else { "" };
        format!("{RANGE}: {s}{suffix}").into_bytes()
    }

    pub fn to_value(&self) -> Vec<u8> {
        self.to_string().into_bytes()
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
        let Some(range_string) = self.ranges.iter().map(|r| r.to_string()).reduce(|accum, next| accum + "," + &next) else {
            return Ok(());
        };
        f.write_fmt(format_args!("{0}={range_string}", self.unit))
    }
}

impl<R: Into<HttpRange>> From<R> for RangeHeader<'static> {
    fn from(value: R) -> Self {
        let mut h = RangeHeader::default();
        h.push(value.into());
        h
    }
}

impl<R: Into<HttpRange>> FromIterator<R> for RangeHeader<'static> {
    fn from_iter<T: IntoIterator<Item = R>>(iter: T) -> Self {
        let mut h = RangeHeader::default();
        h.extend(iter);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_canonical(header: &RangeHeader, expected: &str) {
        assert_eq!(header.to_string(), expected)
    }

    #[test]
    fn first_500() {
        let mut rh = RangeHeader::default();
        rh.push(HttpRange::Range {
            start: 0,
            end: Some(499),
        });
        test_canonical(&rh, "bytes=0-499")
    }

    #[test]
    fn second_500() {
        let mut rh = RangeHeader::default();
        rh.push(HttpRange::Range {
            start: 500,
            end: Some(999),
        });
        test_canonical(&rh, "bytes=500-999")
    }

    #[test]
    fn final_500() {
        let mut rh = RangeHeader::default();
        rh.push(HttpRange::Suffix(500));
        test_canonical(&rh, "bytes=-500")
    }

    #[test]
    fn from_9500() {
        let mut rh = RangeHeader::default();
        rh.push(HttpRange::Range {
            start: 9500,
            end: None,
        });
        test_canonical(&rh, "bytes=9500-")
    }

    #[test]
    fn first_last() {
        let mut rh = RangeHeader::default();
        rh.push(HttpRange::Range {
            start: 0,
            end: Some(0),
        });
        rh.push(HttpRange::Suffix(1));
        test_canonical(&rh, "bytes=0-0,-1");
    }

    #[test]
    fn multibytes() {
        let mut rh = RangeHeader::default();
        rh.push(HttpRange::Range {
            start: 500,
            end: Some(600),
        });
        rh.push(HttpRange::Range {
            start: 601,
            end: Some(999),
        });
        test_canonical(&rh, "bytes=500-600,601-999");
    }

    #[test]
    fn multibytes_overlap() {
        let mut rh = RangeHeader::default();
        rh.push(HttpRange::Range {
            start: 500,
            end: Some(700),
        });
        rh.push(HttpRange::Range {
            start: 601,
            end: Some(999),
        });
        test_canonical(&rh, "bytes=500-700,601-999");
    }

    #[test]
    fn from_range_exclusive() {
        let r: HttpRange = (50..100).into();
        assert_eq!(r.to_string(), "50-99")
    }

    #[test]
    fn from_range_inclusive() {
        let r: HttpRange = (50..=100).into();
        assert_eq!(r.to_string(), "50-100")
    }

    #[test]
    fn from_range_lower_unbounded() {
        let r: HttpRange = (..50).into();
        assert_eq!(r.to_string(), "0-49")
    }

    #[test]
    fn from_range_upper_unbounded() {
        let r: HttpRange = (50..).into();
        assert_eq!(r.to_string(), "50-")
    }

    #[test]
    fn from_iter() {
        let r: RangeHeader = vec![0..50, 40..100, 150..200].into_iter().collect();
        assert_eq!(r.to_string(), "bytes=0-49,40-99,150-199")
    }
}
