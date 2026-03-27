//! Compile schemas extracted from Terraform providers into Serde type definitions.

// registry creation
pub mod binding;

// code generator
pub mod emit;

// configuration support for code generation
pub mod config;

/// Utility functions to help testing code generators.
#[doc(hidden)]
pub mod test_utils;
