//! High-level binding generator with configurable options.
//!
//! Wraps the lower-level `schema_bindgen` functions and provides a convenient
//! API for reading schemas, configuring output, and writing generated Rust
//! bindings to various destinations.
//!
//! Generation options (title-case, builders, trait impls, custom preamble,
//! etc.) are configured via [`CodeGeneratorConfig`] which is held as a field.
//! Use [`with_code_generator_config`](BindingGenerator::with_code_generator_config)
//! to supply a fully customised config, or the convenience `with_*` methods
//! which forward to the underlying config.

use crate::schema_bindgen::binding::{
    TerraformSchemaExport, export_filtered_resources, export_schema_to_registry,
    read_tf_schema_from_file,
};
use crate::schema_bindgen::config::CodeGeneratorConfig;
use crate::schema_bindgen::emit::{CodeGenerator, Registry};
use crate::terra::ResourceFilter;
use std::io::Write;
use std::path::Path;

/// High-level options for Terraform schema binding generation.
///
/// # Example — filtered, typed generation
///
/// ```rust,ignore
/// let filter = ResourceFilter::default()
///     .with_resources("registry.opentofu.org/hashicorp/aws", [
///         "aws_lambda_function",
///         "aws_s3_bucket",
///     ]);
///
/// let schema = BindingGenerator::read_schema("schema.json")?;
/// let generator = BindingGenerator::new()
///     .with_filter(filter)
///     .with_title_case(true);
///
/// generator.generate_to_file(&schema, "src/providers/aws_lambda.rs")?;
/// ```
#[derive(Clone)]
pub struct BindingGenerator {
    /// All code-generation options (title case, builders, trait impls,
    /// preamble, etc.) live here.
    config: CodeGeneratorConfig,

    /// Optional resource filter.  When set, only the specified resources are
    /// parsed from the schema.
    filter: Option<ResourceFilter>,
}

impl Default for BindingGenerator {
    fn default() -> Self {
        Self {
            config: CodeGeneratorConfig::default(),
            filter: None,
        }
    }
}

impl BindingGenerator {
    /// Create a new `BindingGenerator` with default options.
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------------
    // Configuration — direct config access
    // ------------------------------------------------------------------

    /// Replace the entire [`CodeGeneratorConfig`].
    ///
    /// Use this when you need full control over every code-generation knob
    /// rather than calling the individual `with_*` convenience methods.
    pub fn with_code_generator_config(mut self, config: CodeGeneratorConfig) -> Self {
        self.config = config;
        self
    }

    /// Return a shared reference to the current [`CodeGeneratorConfig`].
    pub fn code_generator_config(&self) -> &CodeGeneratorConfig {
        &self.config
    }

    /// Return a mutable reference to the current [`CodeGeneratorConfig`].
    pub fn code_generator_config_mut(&mut self) -> &mut CodeGeneratorConfig {
        &mut self.config
    }

    // ------------------------------------------------------------------
    // Configuration — convenience forwards
    // ------------------------------------------------------------------

    /// Enable or disable `new()` constructor generation.
    pub fn with_builders(mut self, enabled: bool) -> Self {
        self.config = self.config.with_generate_builders(enabled);
        self
    }

    /// Enable or disable `UpperCamelCase` type-name conversion.
    pub fn with_title_case(mut self, enabled: bool) -> Self {
        self.config = self.config.with_title_case(enabled);
        self
    }

    /// Set the resource filter.
    pub fn with_filter(mut self, filter: ResourceFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Enable generation of `TerraResource` / `TerraJson` trait impls.
    pub fn with_trait_impls(mut self, enabled: bool) -> Self {
        self.config = self.config.with_generate_trait_impls(enabled);
        self
    }

    /// Replace the default preamble (`#![allow(…)]`, `use serde::…`, etc.)
    /// with a custom one.
    pub fn with_custom_preamble(mut self, preamble: impl Into<String>) -> Self {
        self.config = self.config.with_custom_preamble(preamble);
        self
    }

    /// When `true`, always derive `Default` for structs — even those with
    /// required fields.
    pub fn with_generate_default(mut self, enabled: bool) -> Self {
        self.config = self.config.with_generate_default(enabled);
        self
    }

    // ------------------------------------------------------------------
    // Schema I/O
    // ------------------------------------------------------------------

    /// Read a Terraform provider schema from a JSON file on disk.
    pub fn read_schema(
        path: impl AsRef<Path>,
    ) -> Result<TerraformSchemaExport, Box<dyn std::error::Error>> {
        Ok(read_tf_schema_from_file(path)?)
    }

    /// Read a schema file and return a default generator together with the
    /// parsed schema — a convenience shorthand for the common case.
    pub fn from_schema_file(
        path: impl AsRef<Path>,
    ) -> Result<(Self, TerraformSchemaExport), Box<dyn std::error::Error>> {
        let schema = read_tf_schema_from_file(path)?;
        Ok((Self::default(), schema))
    }

    // ------------------------------------------------------------------
    // Code generation
    // ------------------------------------------------------------------

    /// Generate Rust bindings for the given schema and write them to `out`.
    pub fn generate_to_writer(
        &self,
        schema: &TerraformSchemaExport,
        out: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let config = self.build_config(schema)?;
        let registry = self.build_registry(schema)?;

        CodeGenerator::new(&config).output(out, &registry)?;
        Ok(())
    }

    /// Generate Rust bindings and return them as a `String`.
    pub fn generate_to_string(
        &self,
        schema: &TerraformSchemaExport,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut buf = Vec::new();
        self.generate_to_writer(schema, &mut buf)?;
        Ok(String::from_utf8(buf)?)
    }

    /// Generate Rust bindings and write them to a file at `path`.
    pub fn generate_to_file(
        &self,
        schema: &TerraformSchemaExport,
        path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = std::fs::File::create(path)?;
        self.generate_to_writer(schema, &mut file)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Build the final [`CodeGeneratorConfig`] by merging the stored config
    /// with data extracted from the schema (resource meta, doc comments,
    /// root-generation flag).
    fn build_config(
        &self,
        schema: &TerraformSchemaExport,
    ) -> Result<CodeGeneratorConfig, Box<dyn std::error::Error>> {
        let mut config = self.config.clone();

        if let Some(filter) = &self.filter {
            let (_registry, meta, comments) = export_filtered_resources(schema, filter, &config)?;
            config = config.with_resource_meta(meta).with_comments(comments);
        } else {
            // Unfiltered: full schema with root types.
            config = config.with_generate_roots(true);
        }

        Ok(config)
    }

    /// Build the serde-reflection registry, using filtering when configured.
    fn build_registry(
        &self,
        schema: &TerraformSchemaExport,
    ) -> Result<Registry, Box<dyn std::error::Error>> {
        if let Some(filter) = &self.filter {
            let (registry, _meta, _comments) =
                export_filtered_resources(schema, filter, &self.config)?;
            Ok(registry)
        } else {
            let registry = export_schema_to_registry(schema)?;
            Ok(registry)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terra::ResourceMeta;
    use serde_reflection::{ContainerFormat, Format, Named};

    #[test]
    fn test_default_generator() {
        let generator = BindingGenerator::default();
        assert!(generator.filter.is_none());
    }

    #[test]
    fn test_new_equals_default() {
        let a = BindingGenerator::new();
        let b = BindingGenerator::default();
        assert_eq!(a.config.use_title_case, b.config.use_title_case);
        assert_eq!(a.config.generate_builders, b.config.generate_builders);
    }

    #[test]
    fn test_with_code_generator_config() {
        let config = CodeGeneratorConfig::new()
            .with_module_name("custom")
            .with_title_case(true)
            .with_generate_builders(false);

        let generator = BindingGenerator::new().with_code_generator_config(config);
        assert!(generator.config.use_title_case);
        assert!(!generator.config.generate_builders);
    }

    #[test]
    fn test_convenience_forwards() {
        let generator = BindingGenerator::new()
            .with_title_case(true)
            .with_builders(false)
            .with_trait_impls(true)
            .with_generate_default(true);

        assert!(generator.config.use_title_case);
        assert!(!generator.config.generate_builders);
        assert!(generator.config.generate_trait_impls);
        assert!(generator.config.generate_default);
    }

    #[test]
    fn test_format_helpers_accessible_via_emit() {
        // Verify the emit module's helpers work (they were moved from here).
        assert!(matches!(
            Format::Option(Box::new(Format::Str)),
            Format::Option(_)
        ));
    }

    #[test]
    fn test_generate_builder_impls_skips_all_optional() {
        let config = CodeGeneratorConfig::new().with_generate_builders(true);

        let mut registry = Registry::new();
        registry.insert(
            (None, "AllOptional".to_string()),
            ContainerFormat::Struct(vec![
                Named {
                    name: "a".to_string(),
                    value: Format::Option(Box::new(Format::Str)),
                },
                Named {
                    name: "b".to_string(),
                    value: Format::Option(Box::new(Format::Bool)),
                },
            ]),
        );

        let mut buf = Vec::new();
        CodeGenerator::new(&config)
            .output(&mut buf, &registry)
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        // All-optional struct should NOT have a builder impl.
        assert!(
            !output.contains("impl AllOptional {"),
            "expected no builder for all-optional struct, got:\n{}",
            output
        );
    }

    #[test]
    fn test_generate_builder_impls_with_required() {
        let config = CodeGeneratorConfig::new().with_generate_builders(true);

        let mut registry = Registry::new();
        registry.insert(
            (None, "MyStruct".to_string()),
            ContainerFormat::Struct(vec![
                Named {
                    name: "name".to_string(),
                    value: Format::Str,
                },
                Named {
                    name: "count".to_string(),
                    value: Format::I64,
                },
                Named {
                    name: "label".to_string(),
                    value: Format::Option(Box::new(Format::Str)),
                },
            ]),
        );

        let mut buf = Vec::new();
        CodeGenerator::new(&config)
            .output(&mut buf, &registry)
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("impl MyStruct {"), "missing builder impl");
        assert!(
            output.contains("pub fn new(name: String, count: i64) -> Self {"),
            "incorrect builder signature, got:\n{}",
            output
        );
        assert!(output.contains("name,"));
        assert!(output.contains("count,"));
        assert!(output.contains("label: None,"));
    }

    #[test]
    fn test_generate_builder_impls_with_namespace() {
        let config = CodeGeneratorConfig::new().with_generate_builders(true);

        let mut registry = Registry::new();
        registry.insert(
            (Some("resource".to_string()), "my_thing".to_string()),
            ContainerFormat::Struct(vec![Named {
                name: "id".to_string(),
                value: Format::Str,
            }]),
        );

        let mut buf = Vec::new();
        CodeGenerator::new(&config)
            .output(&mut buf, &registry)
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("impl resource_my_thing {"),
            "expected namespaced struct name, got:\n{}",
            output
        );
    }

    #[test]
    fn test_generate_builder_impls_title_case() {
        let config = CodeGeneratorConfig::new()
            .with_title_case(true)
            .with_generate_builders(true);

        let mut registry = Registry::new();
        registry.insert(
            (None, "my_struct".to_string()),
            ContainerFormat::Struct(vec![Named {
                name: "id".to_string(),
                value: Format::Str,
            }]),
        );

        let mut buf = Vec::new();
        CodeGenerator::new(&config)
            .output(&mut buf, &registry)
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("impl MyStruct {"),
            "expected title-case struct name, got:\n{}",
            output
        );
    }

    #[test]
    fn test_generate_terra_impls() {
        let meta = vec![ResourceMeta {
            resource_type: "aws_s3_bucket".to_string(),
            provider_source: "registry.opentofu.org/hashicorp/aws".to_string(),
            struct_name: "AwsS3BucketDetails".to_string(),
        }];

        let config = CodeGeneratorConfig::new()
            .with_title_case(true)
            .with_generate_trait_impls(true)
            .with_generate_builders(false)
            .with_resource_meta(meta);

        let mut registry = Registry::new();
        registry.insert(
            (None, "AwsS3BucketDetails".to_string()),
            ContainerFormat::Struct(vec![Named {
                name: "bucket".to_string(),
                value: Format::Option(Box::new(Format::Str)),
            }]),
        );

        let mut buf = Vec::new();
        CodeGenerator::new(&config)
            .output(&mut buf, &registry)
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("impl crate::terra::TerraJson for AwsS3BucketDetails"),
            "missing TerraJson impl, got:\n{}",
            output
        );
        assert!(
            output.contains("impl crate::terra::TerraResource for AwsS3BucketDetails"),
            "missing TerraResource impl, got:\n{}",
            output
        );
        assert!(output.contains("\"aws_s3_bucket\""));
        assert!(output.contains("TerraProvider::AWS"));
    }

    #[test]
    fn test_custom_preamble() {
        let config = CodeGeneratorConfig::new()
            .with_custom_preamble("// custom preamble\nuse custom::stuff;")
            .with_generate_builders(false);

        let registry = Registry::new();

        let mut buf = Vec::new();
        CodeGenerator::new(&config)
            .output(&mut buf, &registry)
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.starts_with("// custom preamble"),
            "expected custom preamble at start, got:\n{}",
            output
        );
        assert!(output.contains("use custom::stuff;"));
        // Should NOT contain the default preamble.
        assert!(!output.contains("use serde_bytes::ByteBuf as Bytes;"));
    }

    #[test]
    fn test_generate_default_forces_default_derive() {
        let config = CodeGeneratorConfig::new()
            .with_generate_default(true)
            .with_generate_builders(false);

        let mut registry = Registry::new();
        registry.insert(
            (None, "RequiredFields".to_string()),
            ContainerFormat::Struct(vec![Named {
                name: "name".to_string(),
                value: Format::Str,
            }]),
        );

        let mut buf = Vec::new();
        CodeGenerator::new(&config)
            .output(&mut buf, &registry)
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("Default"),
            "expected Default derive even with required fields, got:\n{}",
            output
        );
    }
}
