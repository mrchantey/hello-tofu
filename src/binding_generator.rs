//! High-level binding generator with configurable options.
//!
//! Wraps the lower-level `schema_bindgen` functions and provides a convenient
//! API for reading schemas, configuring output, and writing generated Rust
//! bindings to various destinations.

use crate::schema_bindgen::binding::{
    TerraformSchemaExport, export_filtered_resources, export_schema_to_registry,
    read_tf_schema_from_file,
};
use crate::schema_bindgen::config::CodeGeneratorConfig;
use crate::schema_bindgen::emit::{CodeGenerator, Registry};
use crate::terra::{ResourceFilter, ResourceMeta};
use heck::ToUpperCamelCase;
use serde_reflection::{ContainerFormat, Format};
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
pub struct BindingGenerator {
    /// Whether to generate `new()` constructors for types that have required
    /// (non-`Option`) fields.
    pub generate_builders: bool,

    /// When `true`, convert all generated type names from `snake_case` to
    /// `UpperCamelCase` using the `heck` crate.
    pub use_title_case: bool,

    /// Optional resource filter.  When set, only the specified resources are
    /// parsed from the schema and root enum / config types are skipped.
    pub filter: Option<ResourceFilter>,

    /// When `true`, generate `TerraResource` and `TerraJson` trait
    /// implementations for each resource struct.
    pub generate_trait_impls: bool,

    /// Optional custom preamble to prepend to the generated code instead of
    /// the default one.
    pub custom_preamble: Option<String>,
}

impl Default for BindingGenerator {
    fn default() -> Self {
        Self {
            generate_builders: true,
            use_title_case: false,
            filter: None,
            generate_trait_impls: false,
            custom_preamble: None,
        }
    }
}

impl BindingGenerator {
    /// Create a new `BindingGenerator` with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable `new()` constructor generation.
    pub fn with_builders(mut self, enabled: bool) -> Self {
        self.generate_builders = enabled;
        self
    }

    /// Enable or disable `UpperCamelCase` type-name conversion.
    pub fn with_title_case(mut self, enabled: bool) -> Self {
        self.use_title_case = enabled;
        self
    }

    /// Set the resource filter.
    pub fn with_filter(mut self, filter: ResourceFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Enable generation of `TerraResource` / `TerraJson` trait impls.
    pub fn with_trait_impls(mut self, enabled: bool) -> Self {
        self.generate_trait_impls = enabled;
        self
    }

    /// Replace the default preamble (`#![allow(…)]`, `use serde::…`, etc.)
    /// with a custom one.
    pub fn with_custom_preamble(mut self, preamble: impl Into<String>) -> Self {
        self.custom_preamble = Some(preamble.into());
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
        let (registry, meta) = self.build_registry(schema)?;

        // Generate base code into a buffer so we can post-process it.
        let mut buf = Vec::new();
        self.emit_code(&registry, &mut buf)?;
        let mut code = String::from_utf8(buf)?;

        // Replace preamble when a custom one is provided.
        if let Some(preamble) = &self.custom_preamble {
            // The default preamble occupies the first few lines up to (and
            // including) the first blank line.
            if let Some(pos) = code.find("\n\n") {
                code = format!("{}\n{}", preamble, &code[pos + 2..]);
            }
        }

        // Optionally append `new()` constructors for structs with required fields.
        if self.generate_builders {
            code.push_str(&self.generate_builder_impls(&registry));
        }

        // Optionally append TerraResource / TerraJson trait impls.
        if self.generate_trait_impls {
            code.push_str(&self.generate_terra_impls(&meta));
        }

        out.write_all(code.as_bytes())?;
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

    /// Build the serde-reflection registry, using filtering when configured.
    fn build_registry(
        &self,
        schema: &TerraformSchemaExport,
    ) -> Result<(Registry, Vec<ResourceMeta>), Box<dyn std::error::Error>> {
        if let Some(filter) = &self.filter {
            // Filtered path: only selected resources, no root types.
            let (registry, mut meta) = export_filtered_resources(schema, filter)?;

            // When title-case is active, update the struct names in meta.
            if self.use_title_case {
                for m in &mut meta {
                    m.struct_name = m.struct_name.to_upper_camel_case();
                }
            }

            Ok((registry, meta))
        } else {
            // Unfiltered: full schema with root types.
            let registry = export_schema_to_registry(schema)?;
            Ok((registry, Vec::new()))
        }
    }

    /// Run the serde code generator with our configuration.
    fn emit_code(
        &self,
        registry: &Registry,
        out: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let config = CodeGeneratorConfig::new("default".to_string())
            .with_title_case(self.use_title_case)
            .with_generate_roots(self.filter.is_none());

        CodeGenerator::new(&config).output(out, registry)?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Builder / constructor generation
    // ------------------------------------------------------------------

    /// Iterate over all `Struct` entries in the registry and emit an `impl`
    /// block with a `pub fn new(…)` constructor for every struct that
    /// contains at least one required (non-`Option`) field.
    fn generate_builder_impls(&self, registry: &Registry) -> String {
        let mut output = String::new();

        for ((ns, name), format) in registry {
            if let ContainerFormat::Struct(fields) = format {
                let required_fields: Vec<_> = fields
                    .iter()
                    .filter(|f| !is_optional_format(&f.value))
                    .collect();

                // Nothing to do when every field is already optional.
                if required_fields.is_empty() {
                    continue;
                }

                // Reconstruct the struct name exactly as the emitter writes it.
                let raw_name = match ns {
                    Some(ns_val) => format!("{}_{}", ns_val, name),
                    None => name.clone(),
                };
                let struct_name = if self.use_title_case {
                    raw_name.to_upper_camel_case()
                } else {
                    raw_name
                };

                output.push_str(&format!("impl {} {{\n", struct_name));

                // Build the parameter list from required fields only.
                let params: Vec<String> = required_fields
                    .iter()
                    .map(|f| {
                        format!(
                            "{}: {}",
                            f.name,
                            format_to_type(&f.value, self.use_title_case)
                        )
                    })
                    .collect();

                output.push_str(&format!(
                    "    pub fn new({}) -> Self {{\n",
                    params.join(", ")
                ));
                output.push_str("        Self {\n");

                for field in fields {
                    if is_optional_format(&field.value) {
                        output.push_str(&format!(
                            "            {}: {},\n",
                            field.name,
                            default_value_for(&field.value)
                        ));
                    } else {
                        output.push_str(&format!("            {},\n", field.name));
                    }
                }

                output.push_str("        }\n");
                output.push_str("    }\n");
                output.push_str("}\n\n");
            }
        }

        output
    }

    // ------------------------------------------------------------------
    // TerraResource / TerraJson trait impl generation
    // ------------------------------------------------------------------

    /// Generate `TerraJson` and `TerraResource` impl blocks for every
    /// resource in `meta`.
    fn generate_terra_impls(&self, meta: &[ResourceMeta]) -> String {
        let mut output = String::new();

        for m in meta {
            let provider_const = provider_source_to_const(&m.provider_source);

            // TerraJson impl
            output.push_str(&format!(
                "impl crate::terra::TerraJson for {} {{\n",
                m.struct_name
            ));
            output.push_str("    fn to_json(&self) -> serde_json::Value {\n");
            output.push_str(
                "        serde_json::to_value(self).expect(\"serialization should not fail\")\n",
            );
            output.push_str("    }\n");
            output.push_str("}\n\n");

            // TerraResource impl
            output.push_str(&format!(
                "impl crate::terra::TerraResource for {} {{\n",
                m.struct_name
            ));
            output.push_str(&format!(
                "    fn resource_type(&self) -> &'static str {{ \"{}\" }}\n",
                m.resource_type
            ));
            output.push_str(&format!(
                "    fn provider(&self) -> &'static crate::terra::TerraProvider {{ &crate::terra::TerraProvider::{} }}\n",
                provider_const
            ));
            output.push_str("}\n\n");
        }

        output
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Map a provider source string to the `TerraProvider` constant name.
fn provider_source_to_const(source: &str) -> String {
    let provider_name = source.split('/').last().unwrap_or("unknown");
    provider_name.to_uppercase()
}

/// Returns `true` when the format represents an `Option<T>` field.
fn is_optional_format(format: &Format) -> bool {
    matches!(format, Format::Option(_))
}

/// Map a `serde_reflection::Format` to its Rust source-level type string.
///
/// This intentionally mirrors the `quote_type` logic in `emit.rs` but is kept
/// separate so we do not depend on internal emitter details.
fn format_to_type(format: &Format, title_case: bool) -> String {
    match format {
        Format::TypeName(name) => {
            if title_case {
                name.to_upper_camel_case()
            } else {
                name.clone()
            }
        }
        Format::Unit => "()".to_string(),
        Format::Bool => "bool".to_string(),
        Format::I8 => "i8".to_string(),
        Format::I16 => "i16".to_string(),
        Format::I32 => "i32".to_string(),
        Format::I64 => "i64".to_string(),
        Format::I128 => "i128".to_string(),
        Format::U8 => "u8".to_string(),
        Format::U16 => "u16".to_string(),
        Format::U32 => "u32".to_string(),
        Format::U64 => "u64".to_string(),
        Format::U128 => "u128".to_string(),
        Format::F32 => "f32".to_string(),
        Format::F64 => "f64".to_string(),
        Format::Char => "char".to_string(),
        Format::Str => "String".to_string(),
        Format::Bytes => "Bytes".to_string(),
        Format::Option(inner) => format!("Option<{}>", format_to_type(inner, title_case)),
        Format::Seq(inner) => format!("Vec<{}>", format_to_type(inner, title_case)),
        Format::Map { key, value } => {
            format!(
                "Map<{}, {}>",
                format_to_type(key, title_case),
                format_to_type(value, title_case)
            )
        }
        Format::Tuple(formats) => {
            let types: Vec<_> = formats
                .iter()
                .map(|f| format_to_type(f, title_case))
                .collect();
            format!("({})", types.join(", "))
        }
        Format::TupleArray { content, size } => {
            format!("[{}; {}]", format_to_type(content, title_case), size)
        }
        Format::Variable(_) => panic!("unexpected variable format in type conversion"),
    }
}

/// Return a Rust expression that produces the natural default value for a
/// given format (used for optional fields in generated constructors).
fn default_value_for(format: &Format) -> String {
    match format {
        Format::Option(_) => "None".to_string(),
        Format::Seq(_) => "Vec::new()".to_string(),
        Format::Str => "String::new()".to_string(),
        Format::Bool => "false".to_string(),
        Format::I8 | Format::I16 | Format::I32 | Format::I64 | Format::I128 => "0".to_string(),
        Format::U8 | Format::U16 | Format::U32 | Format::U64 | Format::U128 => "0".to_string(),
        Format::F32 | Format::F64 => "0.0".to_string(),
        Format::Map { .. } => "Map::new()".to_string(),
        _ => "Default::default()".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_generator() {
        let generator = BindingGenerator::default();
        assert!(generator.generate_builders);
        assert!(!generator.use_title_case);
        assert!(generator.filter.is_none());
    }

    #[test]
    fn test_new_equals_default() {
        let a = BindingGenerator::new();
        let b = BindingGenerator::default();
        assert_eq!(a.generate_builders, b.generate_builders);
        assert_eq!(a.use_title_case, b.use_title_case);
    }

    #[test]
    fn test_format_to_type_primitives() {
        assert_eq!(format_to_type(&Format::Str, false), "String");
        assert_eq!(format_to_type(&Format::Bool, false), "bool");
        assert_eq!(format_to_type(&Format::I64, false), "i64");
        assert_eq!(format_to_type(&Format::U32, false), "u32");
        assert_eq!(format_to_type(&Format::F64, false), "f64");
    }

    #[test]
    fn test_format_to_type_composites() {
        assert_eq!(
            format_to_type(&Format::Option(Box::new(Format::Str)), false),
            "Option<String>"
        );
        assert_eq!(
            format_to_type(&Format::Seq(Box::new(Format::I64)), false),
            "Vec<i64>"
        );
        assert_eq!(
            format_to_type(
                &Format::Map {
                    key: Box::new(Format::Str),
                    value: Box::new(Format::Bool),
                },
                false
            ),
            "Map<String, bool>"
        );
    }

    #[test]
    fn test_default_value_for() {
        assert_eq!(
            default_value_for(&Format::Option(Box::new(Format::Str))),
            "None"
        );
        assert_eq!(
            default_value_for(&Format::Seq(Box::new(Format::I64))),
            "Vec::new()"
        );
        assert_eq!(default_value_for(&Format::Str), "String::new()");
        assert_eq!(default_value_for(&Format::Bool), "false");
        assert_eq!(default_value_for(&Format::I64), "0");
        assert_eq!(default_value_for(&Format::F32), "0.0");
    }

    #[test]
    fn test_is_optional_format() {
        assert!(is_optional_format(&Format::Option(Box::new(Format::Str))));
        assert!(!is_optional_format(&Format::Str));
        assert!(!is_optional_format(&Format::Seq(Box::new(Format::Bool))));
    }

    #[test]
    fn test_generate_builder_impls_skips_all_optional() {
        use serde_reflection::Named;

        let generator = BindingGenerator::new();
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

        let output = generator.generate_builder_impls(&registry);
        assert!(
            output.is_empty(),
            "expected no builder for all-optional struct"
        );
    }

    #[test]
    fn test_generate_builder_impls_with_required() {
        use serde_reflection::Named;

        let generator = BindingGenerator::new();
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

        let output = generator.generate_builder_impls(&registry);
        assert!(output.contains("impl MyStruct {"));
        assert!(output.contains("pub fn new(name: String, count: i64) -> Self {"));
        assert!(output.contains("name,"));
        assert!(output.contains("count,"));
        assert!(output.contains("label: None,"));
    }

    #[test]
    fn test_generate_builder_impls_with_namespace() {
        use serde_reflection::Named;

        let generator = BindingGenerator::new();
        let mut registry = Registry::new();
        registry.insert(
            (Some("resource".to_string()), "my_thing".to_string()),
            ContainerFormat::Struct(vec![Named {
                name: "id".to_string(),
                value: Format::Str,
            }]),
        );

        let output = generator.generate_builder_impls(&registry);
        assert!(
            output.contains("impl resource_my_thing {"),
            "expected namespaced struct name, got: {}",
            output
        );
    }

    #[test]
    fn test_generate_builder_impls_title_case() {
        use serde_reflection::Named;

        let generator = BindingGenerator::new().with_title_case(true);
        let mut registry = Registry::new();
        registry.insert(
            (None, "my_struct".to_string()),
            ContainerFormat::Struct(vec![Named {
                name: "id".to_string(),
                value: Format::Str,
            }]),
        );

        let output = generator.generate_builder_impls(&registry);
        assert!(
            output.contains("impl MyStruct {"),
            "expected title-case struct name, got: {}",
            output
        );
    }

    #[test]
    fn test_provider_source_to_const() {
        assert_eq!(
            provider_source_to_const("registry.opentofu.org/hashicorp/aws"),
            "AWS"
        );
        assert_eq!(
            provider_source_to_const("registry.opentofu.org/cloudflare/cloudflare"),
            "CLOUDFLARE"
        );
    }

    #[test]
    fn test_generate_terra_impls() {
        let generator = BindingGenerator::new().with_trait_impls(true);
        let meta = vec![ResourceMeta {
            resource_type: "aws_s3_bucket".to_string(),
            provider_source: "registry.opentofu.org/hashicorp/aws".to_string(),
            struct_name: "AwsS3BucketDetails".to_string(),
        }];

        let output = generator.generate_terra_impls(&meta);
        assert!(output.contains("impl crate::terra::TerraJson for AwsS3BucketDetails"));
        assert!(output.contains("impl crate::terra::TerraResource for AwsS3BucketDetails"));
        assert!(output.contains("\"aws_s3_bucket\""));
        assert!(output.contains("TerraProvider::AWS"));
    }
}
