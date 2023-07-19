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
            .and_then(|v| v.to_str().ok())
    }

    fn content_range_str(&self) -> Option<&str> {
        self.headers()
            .get("Content-Range")
            .and_then(|v| v.to_str().ok())
    }

    fn body(self) -> Result<bytes::Bytes, Box<dyn Error>> {
        self.bytes().map_err(|e| {
            let e: Box<dyn Error> = Box::new(e);
            e
        })
    }
}
