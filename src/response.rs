use std::{
    collections::{btree_map::Entry, BTreeMap},
    io::{self, Cursor, Read, Seek, SeekFrom},
};

use http_content_range::ContentRange;
use httparse::{parse_headers, EMPTY_HEADER};
use rope_rd::sparse::{Part, Spacer};
use rope_rd::util::abs_position;
use rope_rd::Node;
use thiserror::Error;

pub use bytes::{Buf, Bytes};

const BYTERANGES: &str = "multipart/byteranges";

/// A component part of a 206 response.
#[derive(Debug, Clone)]
pub struct ResponsePart {
    content_type: String,
    content_range: ContentRange,
    data: Bytes,
}

impl ResponsePart {
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// The offset and length of the part according to the `Content-Range` header.
    ///
    /// If the range was unsatisfied or the content range was not parseable,
    /// return [None].
    pub fn offset_len(&self) -> Option<(usize, usize)> {
        offset_len(&self.content_range)
    }

    /// The size according to the `Content-Range` header.
    ///
    /// [None] if the header did not express that information.
    pub fn total_size(&self) -> Option<usize> {
        match &self.content_range {
            ContentRange::Bytes(r) => Some(r.complete_length as usize),
            _ => None,
        }
    }

    /// The bytes in the part.
    pub fn data(&self) -> &Bytes {
        &self.data
    }
}

fn offset_len(content_range: &ContentRange) -> Option<(usize, usize)> {
    match content_range {
        ContentRange::Bytes(r) => Some((
            r.first_byte as usize,
            (r.last_byte - r.first_byte + 1) as usize,
        )),
        ContentRange::UnboundBytes(r) => Some((
            r.first_byte as usize,
            (r.last_byte - r.first_byte + 1) as usize,
        )),
        ContentRange::Unsatisfied(_r) => None,
        ContentRange::Unknown => None,
    }
}

/// A description of the partial response headers.
///
/// This may be a single part, in which case the `Content-Range` and `Content-Type` values are known,
/// or a `mutipart/byteranges` response, in which case the `boundary` is known.
pub enum PartDesc {
    Single {
        content_range: ContentRange,
        content_type: String,
    },
    Multi {
        boundary: Vec<u8>,
    },
}

#[derive(Debug, Error)]
pub enum PartialHeaderParseError {
    #[error("Range response could not be satisfied (status 416)")]
    Unsatisfied,
    #[error("Expected response 206, got {0}")]
    NotPartialResponse(u16),
    #[error("No Content-Type header found")]
    NoContentType,
    #[error("No Content-Range header found")]
    NoContentRange,
    #[error("Could not parse Content-Range header: {0}")]
    ContentRangeParse(String),
    #[error(transparent)]
    BodyRead(#[from] Box<dyn std::error::Error>),
}

/// Trait for a response which may be a 206 Partial.
///
/// Implemented for [http::Response](https://docs.rs/http/latest/http/response/struct.Response.html)
/// and [reqwest::blocking::Response](https://docs.rs/reqwest/latest/reqwest/struct.Response.html)
/// behind the relevant feature flags.
pub trait MaybePartialResponse: Sized {
    fn status_code(&self) -> u16;

    /// Value of the response's `Content-Type` header if present.
    fn content_type_str(&self) -> Option<&str>;

    /// Value of the response's `Content-Range` header if present.
    fn content_range_str(&self) -> Option<&str>;

    /// The bytes of the response body.
    // todo: could this error be generic instead?
    fn body(self) -> Result<Bytes, Box<dyn std::error::Error>>;

    /// If the response is a 206 Partial, a description of what type based on the headers.
    fn part_description(&self) -> Result<PartDesc, PartialHeaderParseError> {
        use PartialHeaderParseError::*;
        let status = self.status_code();
        match status {
            416 => Err(Unsatisfied),
            206 => Ok(()),
            n => Err(NotPartialResponse(n)),
        }?;
        let mut s = self.content_type_str().ok_or(NoContentType)?;

        s = s.trim();
        if s.starts_with(BYTERANGES) {
            let boundary_str = s[..BYTERANGES.len() + 1].trim_start()[9..]
                .trim_matches('"')
                .trim_matches('\'');
            let boundary = format!("--{boundary_str}").as_bytes().to_vec();
            Ok(PartDesc::Multi { boundary })
        } else {
            let cr_s = self.content_range_str().ok_or(NoContentRange)?;
            let mut cr = ContentRange::parse(cr_s);
            cr = match cr {
                // ContentRange::Bytes(_) => todo!(),
                // ContentRange::UnboundBytes(_) => todo!(),
                ContentRange::Unsatisfied(_) => unreachable!(),
                ContentRange::Unknown => Err(ContentRangeParse(cr_s.to_owned())),
                _ => Ok(cr),
            }?;
            Ok(PartDesc::Single {
                content_range: cr,
                content_type: s.to_owned(),
            })
        }
    }

    /// If the response is a 206 Partial, an iterator over its [ResponsePart]s.
    fn parts(self) -> Result<Parts, PartialHeaderParseError> {
        Ok(Parts::new(self.part_description()?, self.body()?))
    }

    /// Representation of the whole requested file, with [Read]/[Seek].
    ///
    /// If the response was complete (whether or not that was requested), the whole file will be present.
    ///
    /// If the response was a 206 Partial, only the parts in the response will be the "real" file: the remainder will be null bytes.
    /// This does not take up the memory that the whole file would, as the [SparseBody] generates the filler material on the fly.
    ///
    /// Responses which contain overlapping ranges will cause unexpected behaviour; blame the server.
    fn sparse_body(self) -> Result<SparseBody, SparseBodyError> {
        if self.status_code() == 200 {
            return Ok(SparseBody::full(self.body()?));
        }
        let pv: Result<Vec<ResponsePart>, PartParseError> = self.parts()?.collect();
        Ok(SparseBody::partial(pv?))
    }
}

#[derive(Debug, Error)]
pub enum SparseBodyError {
    #[error(transparent)]
    Header(#[from] PartialHeaderParseError),
    #[error(transparent)]
    Part(#[from] PartParseError),
    #[error(transparent)]
    Body(#[from] Box<dyn std::error::Error>),
}

// Iterator over parts of a 206 Partial response.
pub struct Parts {
    part_desc: PartDesc,
    body: Bytes,
    is_done: bool,
    next_start: usize,
}

impl Parts {
    pub fn new(part_desc: PartDesc, body: Bytes) -> Self {
        let mut next_start = 0;
        let mut is_done = false;
        let boundary = match &part_desc {
            PartDesc::Single { .. } => {
                return Self {
                    part_desc,
                    body,
                    is_done,
                    next_start,
                }
            }
            PartDesc::Multi { boundary } => boundary,
        }
        .as_slice();

        for (start_idx, window) in body.windows(boundary.len()).enumerate() {
            if window != boundary {
                continue;
            }
            let end_idx = start_idx + window.len();
            // +2 accounts for CRLF or --
            let tail_end_idx = end_idx + 2;
            let tail = &body[end_idx..tail_end_idx];
            match tail {
                b"\r\n" => {
                    next_start = tail_end_idx;
                    break;
                }
                b"--" => {
                    is_done = true;
                    break;
                }
                t => panic!("Boundary not followed by CRLF or double hyphen: {t:?}"),
            };
        }
        Self {
            part_desc,
            body,
            is_done,
            next_start,
        }
    }
}

#[derive(Debug, Copy, Clone, Error)]
#[error("Could not parse part headers")]
pub struct PartParseError();

impl Iterator for Parts {
    type Item = Result<ResponsePart, PartParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }
        let boundary = match &self.part_desc {
            PartDesc::Single {
                content_range,
                content_type,
            } => {
                self.is_done = true;
                return Some(Ok(ResponsePart {
                    content_type: content_type.to_string(),
                    content_range: *content_range,
                    data: self.body.clone(),
                }));
            }
            PartDesc::Multi { boundary } => boundary,
        }
        .as_slice();

        for (offset, window) in self.body[self.next_start..]
            .windows(boundary.len())
            .enumerate()
        {
            if window != boundary {
                continue;
            }
            // -2 accounts for CRLF
            let prev_end = offset + self.next_start - 2;

            let slice = self.body.slice(self.next_start..prev_end);

            // +2 accounts for CRLF between boundary and header, or -- to mark end
            self.next_start += offset + boundary.len() + 2;

            self.is_done = match &self.body[self.next_start - 2..self.next_start] {
                b"\r\n" => false,
                b"--" => true,
                t => panic!("Boundary not followed by CRLF or double hyphen: {t:?}"),
            };
            let mut headers = [EMPTY_HEADER; 10];
            let Ok(status) = parse_headers(&slice[..], &mut headers) else {
                return Some(Err(PartParseError()))
            };
            if status.is_partial() {
                return Some(Err(PartParseError()));
            }
            let (idx, heads) = status.unwrap();
            let data = slice.slice(idx..);
            let mut content_range = None;
            let mut content_type = None;
            for head in heads.iter() {
                match head.name.to_lowercase().as_str() {
                    "content-range" => {
                        content_range = Some(ContentRange::parse_bytes(head.value));
                        if content_type.is_some() {
                            break;
                        }
                    }
                    "content-type" => {
                        content_type = Some(head.value.to_owned());
                        if content_range.is_some() {
                            break;
                        }
                    }
                    _ => continue,
                }
                let Some(cr) = content_range else {return Some(Err(PartParseError()))};
                let Some(ct) = content_type else {
                    return Some(Err(PartParseError()))
                };
                let Ok(ct_s) = String::from_utf8(ct) else {
                    return Some(Err(PartParseError()))
                };
                return Some(Ok(ResponsePart {
                    content_type: ct_s,
                    content_range: cr,
                    data,
                }));
            }
        }

        None
    }
}

/// [Read]/[Seek]able [Bytes] wrapper.
struct BytesRS {
    bytes: Bytes,
    position: u64,
}

impl BytesRS {
    fn new(bytes: Bytes) -> Self {
        Self { bytes, position: 0 }
    }
}

impl Read for BytesRS {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut cur = Cursor::new(&self.bytes[..]);
        cur.seek(SeekFrom::Start(self.position))?;
        let n_read = cur.read(buf)?;
        self.position += n_read as u64;
        Ok(n_read)
    }
}

impl Seek for BytesRS {
    fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.position)
    }

    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.position = abs_position(self.position, self.bytes.len() as u64, pos)?;
        Ok(self.position)
    }
}

enum SparseBodyOpt {
    Full(BytesRS),
    Partial(Node<Part<BytesRS>>),
}

impl Read for SparseBodyOpt {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            SparseBodyOpt::Full(b) => b.read(buf),
            SparseBodyOpt::Partial(b) => b.read(buf),
        }
    }
}

impl Seek for SparseBodyOpt {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            SparseBodyOpt::Full(b) => b.seek(pos),
            SparseBodyOpt::Partial(b) => b.seek(pos),
        }
    }
}

/// Struct representing the whole file from which a response was generated.
///
/// If the response contained the whole file, it contains the whole file.
/// If the response was a 206 Partial, it contains a [rope](https://en.wikipedia.org/wiki/Rope_(data_structure))
/// where the fetched parts are in the correct place as reported by the `Content-Range` header
/// and the other parts are null bytes.
///
/// Implements [Read] and [Seek].
pub struct SparseBody(SparseBodyOpt);

impl SparseBody {
    fn full(bytes: Bytes) -> Self {
        SparseBody(SparseBodyOpt::Full(BytesRS::new(bytes)))
    }

    fn partial<T: IntoIterator<Item = ResponsePart>>(parts: T) -> Self {
        make_sparse_body(parts)
    }
}

impl Read for SparseBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for SparseBody {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

fn make_sparse_body<T: IntoIterator<Item = ResponsePart>>(parts: T) -> SparseBody {
    let mut map = BTreeMap::default();

    let mut total_len = 0;
    // offset, len, part
    for p in parts {
        let Some((offset, len)) = p.offset_len() else {
            continue
        };
        if let Some(total) = p.total_size() {
            total_len = total_len.max(total)
        } else {
            total_len = total_len.max(offset + len)
        }
        let tup = (offset, len, p);

        match map.entry(offset) {
            Entry::Occupied(mut e) => {
                let val: &mut (usize, usize, ResponsePart) = e.get_mut();
                if val.1 < len {
                    *val = tup;
                }
            }
            Entry::Vacant(e) => {
                e.insert(tup);
            }
        }
    }

    let mut start_parts = Vec::with_capacity(map.len() * 2 + 1);
    let mut idx = 0;
    for (offset, len, resp) in map.into_values().map(|(o, l, r)| (o as u64, l as u64, r)) {
        if idx < offset {
            let needed_len = offset - idx;
            start_parts.push((idx, Part::Empty(Spacer::new(needed_len))));
        }

        let brs = BytesRS::new(resp.data.clone());

        start_parts.push((offset, Part::Full(brs)));
        idx = offset + len;
    }
    let total_len_64 = total_len as u64;
    if idx < total_len_64 {
        start_parts.push((idx, Part::Empty(Spacer::new(total_len_64 - idx))));
    }
    let n = Node::partition_with_starts(start_parts, total_len_64);
    SparseBody(SparseBodyOpt::Partial(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_impl::{read_text, test_response};

    #[test]
    fn body_50_100() {
        let reference = read_text();
        test_response("bytes=50-100", |resp| {
            let mut bod = resp.sparse_body().unwrap();
            let mut buf = [255; 150];
            bod.read(&mut buf).unwrap();
            assert_eq!(buf[..50], [0; 50]);
            assert_eq!(buf[50..=100], reference[50..=100]);
            assert_eq!(buf[101..150], [0; 49]);
        });
    }

    #[test]
    fn body_3000_() {
        let reference = read_text();
        test_response("bytes=3000-", |resp| {
            let mut bod = resp.sparse_body().unwrap();
            let mut buf = [255; 200];
            bod.seek(SeekFrom::Start(2900)).unwrap();
            bod.read(&mut buf).unwrap();
            assert_eq!(buf[..100], [0; 100]);
            assert_eq!(buf[100..], reference[3000..3100]);
        });
    }

    #[test]
    #[allow(non_snake_case)]
    fn body__100() {
        let reference = read_text();
        test_response("bytes=-100", |resp| {
            let mut bod = resp.sparse_body().unwrap();
            let mut buf = Vec::default();
            bod.seek(SeekFrom::End(-200)).unwrap();
            bod.read_to_end(&mut buf).unwrap();
            assert_eq!(buf[..100], [0; 100]);
            assert_eq!(buf[100..], reference[reference.len() - 100..]);
        });
    }
}
