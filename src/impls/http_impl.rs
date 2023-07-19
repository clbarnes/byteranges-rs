use std::{error::Error, io::Read};

use bytes::Bytes;
pub use http;

use crate::response::MaybePartialResponse;

impl<T: Read> MaybePartialResponse for http::Response<T> {
    fn status_code(&self) -> u16 {
        self.status().as_u16()
    }

    fn content_type_str(&self) -> Option<&str> {
        self.headers()
            .get("Content-Type")
            .map(|v| v.to_str().ok())
            .flatten()
    }

    fn content_range_str(&self) -> Option<&str> {
        self.headers()
            .get("Content-Type")
            .map(|v| v.to_str().ok())
            .flatten()
    }

    fn body(self) -> Result<bytes::Bytes, Box<dyn std::error::Error>> {
        let mut rd = self.into_body();
        let mut buf = Vec::default();
        rd.read_to_end(&mut buf).map_err(|e| {
            let b: Box<dyn Error> = Box::new(e);
            b
        })?;
        Ok(Bytes::from(buf))
    }
}
