//! Auto-generated provider bindings.
//!
//! Each sub-module is gated behind a cargo feature so that only the providers
//! your project needs are compiled.

#[cfg(feature = "providers_aws_lambda")]
pub mod aws_lambda;

#[cfg(feature = "providers_aws_lightsail")]
pub mod aws_lightsail;

#[cfg(feature = "providers_cloudflare_dns")]
pub mod cloudflare_dns;
