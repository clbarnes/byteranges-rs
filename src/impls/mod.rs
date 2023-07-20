#[cfg(feature = "reqwest")]
mod reqwest_impl;
#[cfg(feature = "reqwest")]
pub use reqwest_impl::reqwest;

#[cfg(feature = "http")]
mod http_impl;
#[cfg(feature = "http")]
pub use http_impl::http;

#[cfg(test)]
pub(crate) mod test_impl;
