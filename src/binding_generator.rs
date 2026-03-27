//! High-level binding generator with configurable options.

use crate::schema_bindgen::binding::{
    TerraformSchemaExport, export_schema_to_registry, generate_serde, read_tf_schema_from_file,
};
use crate::schema_bindgen::emit::Registry;
use serde_reflection::{ContainerFormat, Format};
use std::io::Write;
use std::path::Path;

/// High-level options for Terraform schema binding generation.
///
/// Wraps the lower-level schema_bindgen functions and provides a convenient
/// API for reading schemas, configuring output, and writing generated Rust
/// bindings to various destinations.
pub struct BindingGenerator {
    /// Whether to keep the `Default` derive on generated structs.
    ///
    /// The underlying code generator always adds `Default` to struct derives.
    /// When this option is `false`, the `Default` derive is stripped from the
    /// generated output. When `true` (the default), it is left in place.
    pub generate_default: bool,
    /// Whether to generate `new()` constructors for types that have required
    /// (non-`Option`) fields.
    ///
    /// The constructor takes all required fields as parameters and initialises
    /// optional fields to their language-level defaults (`None`, `Vec::new()`,
    /// etc.).
    pub generate_builders: bool,
}

impl Default for BindingGenerator {
    fn default() -> Self {
        Self {
            generate_default: true,
            generate_builders: true,
        }
    }
}

impl BindingGenerator {
    /// Create a new `BindingGenerator` with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read a Terraform provider schema from a JSON file on disk.
    pub fn read_schema(
        path: impl AsRef<Path>,
    ) -> Result<TerraformSchemaExport, Box<dyn std::error::Error>> {
        Ok(read_tf_schema_from_file(path)?)
    }

    /// Read a schema file and return a default generator together with the
    /// parsed schema – a convenience shorthand for the common case.
    pub fn from_schema_file(
        path: impl AsRef<Path>,
    ) -> Result<(Self, TerraformSchemaExport), Box<dyn std::error::Error>> {
        let schema = read_tf_schema_from_file(path)?;
        Ok((Self::default(), schema))
    }

    /// Generate Rust bindings for the given schema and write them to `out`.
    pub fn generate_to_writer(
        &self,
        schema: &TerraformSchemaExport,
        out: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let registry = export_schema_to_registry(schema)?;

        // Generate base code into a buffer so we can post-process it.
        let mut buf = Vec::new();
        generate_serde("default", &mut buf, &registry)?;
        let mut code = String::from_utf8(buf)?;

        // Optionally strip the `Default` derive that the emitter always adds.
        if !self.generate_default {
            code = code.replace(", Default", "");
        }

        // Optionally append `new()` constructors for structs with required fields.
        if self.generate_builders {
            code.push_str(&self.generate_builder_impls(&registry));
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
    // Builder / constructor generation
    // ------------------------------------------------------------------

    /// Iterate over all `Struct` entries in the registry and emit an `impl`
    /// block with a `pub fn new(...)` constructor for every struct that
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
                let struct_name = match ns {
                    Some(ns_val) => format!("{}_{}", ns_val, name),
                    None => name.clone(),
                };

                output.push_str(&format!("impl {} {{\n", struct_name));

                // Build the parameter list from required fields only.
                let params: Vec<String> = required_fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, format_to_type(&f.value)))
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
}

// ------------------------------------------------------------------
// Helper functions
// ------------------------------------------------------------------

/// Returns `true` when the format represents an `Option<T>` field.
fn is_optional_format(format: &Format) -> bool {
    matches!(format, Format::Option(_))
}

/// Map a `serde_reflection::Format` to its Rust source-level type string.
///
/// This intentionally mirrors the `quote_type` logic in `emit.rs` but is kept
/// separate so we do not depend on internal emitter details.
fn format_to_type(format: &Format) -> String {
    match format {
        Format::TypeName(name) => name.clone(),
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
        Format::Option(inner) => format!("Option<{}>", format_to_type(inner)),
        Format::Seq(inner) => format!("Vec<{}>", format_to_type(inner)),
        Format::Map { key, value } => {
            format!("Map<{}, {}>", format_to_type(key), format_to_type(value))
        }
        Format::Tuple(formats) => {
            let types: Vec<_> = formats.iter().map(|f| format_to_type(f)).collect();
            format!("({})", types.join(", "))
        }
        Format::TupleArray { content, size } => {
            format!("[{}; {}]", format_to_type(content), size)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_generator() {
        let generator = BindingGenerator::default();
        assert!(generator.generate_default);
        assert!(generator.generate_builders);
    }

    #[test]
    fn test_new_equals_default() {
        let a = BindingGenerator::new();
        let b = BindingGenerator::default();
        assert_eq!(a.generate_default, b.generate_default);
        assert_eq!(a.generate_builders, b.generate_builders);
    }

    #[test]
    fn test_format_to_type_primitives() {
        assert_eq!(format_to_type(&Format::Str), "String");
        assert_eq!(format_to_type(&Format::Bool), "bool");
        assert_eq!(format_to_type(&Format::I64), "i64");
        assert_eq!(format_to_type(&Format::U32), "u32");
        assert_eq!(format_to_type(&Format::F64), "f64");
    }

    #[test]
    fn test_format_to_type_composites() {
        assert_eq!(
            format_to_type(&Format::Option(Box::new(Format::Str))),
            "Option<String>"
        );
        assert_eq!(
            format_to_type(&Format::Seq(Box::new(Format::I64))),
            "Vec<i64>"
        );
        assert_eq!(
            format_to_type(&Format::Map {
                key: Box::new(Format::Str),
                value: Box::new(Format::Bool),
            }),
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
}
