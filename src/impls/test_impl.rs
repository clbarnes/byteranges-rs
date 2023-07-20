use bytes::Bytes;
use httparse::EMPTY_HEADER;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

use httparse::{Header, Response};

use crate::response::MaybePartialResponse;

pub fn read_text() -> Vec<u8> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("data");
    p.push("lorem.txt");
    let mut v = Vec::default();
    let mut f = fs::File::open(p).unwrap();
    f.read_to_end(&mut v).unwrap();
    v
}

fn read_response(fname: &str) -> Vec<u8> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("data");
    p.push("response");
    p.push(format!("{fname}.http1"));
    let mut v = Vec::default();
    let mut f = fs::File::open(p).unwrap();
    f.read_to_end(&mut v).unwrap();
    v
}

pub fn header_buf() -> [Header<'static>; 64] {
    [EMPTY_HEADER; 64]
}

pub struct DummyResponse<'a> {
    pub response: Response<'a, 'a>,
    body: &'a [u8],
}

impl<'a> DummyResponse<'a> {
    fn new(hbuf: &'a mut [Header<'a>], buf: &'a [u8]) -> Self {
        let mut response = Response::new(hbuf);
        let offset = match response
            .parse(buf)
            .expect("DummyResponse could not parse response bytes")
        {
            httparse::Status::Complete(c) => c,
            httparse::Status::Partial => panic!("partial parse"),
        };
        let body = &buf[offset..];
        Self { response, body }
    }
}

pub fn test_response<T, F: FnOnce(DummyResponse) -> T>(fname: &str, test_fn: F) -> T {
    let mut hbuf = header_buf();
    let mut buf = read_response(fname);
    let tr = DummyResponse::new(&mut hbuf, &mut buf);
    test_fn(tr)
}

impl<'a> MaybePartialResponse for DummyResponse<'a> {
    fn status_code(&self) -> u16 {
        self.response.code.unwrap()
    }

    fn content_type_str(&self) -> Option<&str> {
        for h in self.response.headers.iter() {
            if h.name.to_lowercase() == "content-type" {
                return Some(std::str::from_utf8(h.value).unwrap());
            }
        }
        return None;
    }

    fn content_range_str(&self) -> Option<&str> {
        for h in self.response.headers.iter() {
            if h.name.to_lowercase() == "content-range" {
                return Some(std::str::from_utf8(h.value).unwrap());
            }
        }
        return None;
    }

    fn body(self) -> Result<Bytes, Box<dyn std::error::Error>> {
        Ok(Bytes::from_iter(self.body.iter().cloned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_get_status() {
        test_response("bytes=-100", |r| {
            let status = r.status_code();
            assert_eq!(status, 206);
        })
    }

    #[test]
    fn can_get_body() {
        test_response("bytes=50-100", |r| {
            let b = r.body().expect("body() failed");
            assert_eq!(b.len(), 51)
        })
    }
}
