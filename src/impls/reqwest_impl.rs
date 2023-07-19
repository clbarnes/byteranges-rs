use std::error::Error;

pub use reqwest;
use reqwest::blocking::Response;

impl crate::response::MaybePartialResponse for Response {
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
            .get("Content-Range")
            .map(|v| v.to_str().ok())
            .flatten()
    }

    fn body(self) -> Result<bytes::Bytes, Box<dyn Error>> {
        self.bytes().map_err(|e| {
            let e: Box<dyn Error> = Box::new(e);
            e
        })
    }
}
