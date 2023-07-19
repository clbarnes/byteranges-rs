//! 1. Implement [response::MaybePartialResponse] for the response type in your HTTP client library (possibly using a newtype).
//! 2. Use [request::RangeHeader] to collect [request::HttpRange]s (conveniently constructed from anything implementing [std::ops::RangeBounds]) and convert into the string value for the `Range` header
//! 3. Send off a request with that header.
//! 4. Use [response::MaybePartialResponse::sparse_body] to get a [std::io::Read]/[std::io::Seek] representation of the whole remote file. If the response had `Content-Range`s, those ranges will be the fetched data, and the rest will be null bytes.
//!
//! Assumes that the server has not sent back any overlapping ranges,
//! and that the returned byteranges have the unit `"bytes"`.

pub mod request;

pub mod response;

mod impls;
pub use impls::*;

/// variant_from_data!(EnumType, VariantName, DataType)
///
/// adds `From<D>` for an enum with a variant containing D
///
/// N.B. this is also handled by enum_delegate::implement
#[macro_export]
macro_rules! variant_from_data {
    ($enum:ty, $variant:ident, $data_type:ty) => {
        impl std::convert::From<$data_type> for $enum {
            fn from(c: $data_type) -> Self {
                <$enum>::$variant(c)
            }
        }
    };
}
