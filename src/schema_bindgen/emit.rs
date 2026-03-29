//!
//! Stripped-down version of serde-reflection's code generator for Rust.
//!
//! Main changes from upstream:
//! - Qualified names (`(Option<String>, String)`) for Registry entries
//! - Smart `Default` derive: only added when **all** fields are optional
//!   (or when `generate_default` is enabled in the config)
//! - Optional `UpperCamelCase` conversion for generated type names (via `heck`)
//! - Inline builder (`new()`) generation for structs with required fields
//! - Inline `TerraResource` / `TerraJson` trait impl generation
//! - Custom preamble support
//!

use crate::schema_bindgen::config::CodeGeneratorConfig;
use heck::ToUpperCamelCase;
use serde_generate::indent::{IndentConfig, IndentedWriter};
use serde_reflection::{ContainerFormat, Format, Named, VariantFormat};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Result, Write};

/// A map of container formats indexed by a qualified name.
pub type QualifiedName = (Option<String>, String);
pub type Registry = BTreeMap<QualifiedName, ContainerFormat>;

/// Main configuration object for code-generation in Rust.
pub struct CodeGenerator<'a> {
    /// Language-independent configuration.
    config: &'a CodeGeneratorConfig,
    /// Which derive macros should be added (independently from serialization).
    derive_macros: Vec<String>,
    /// Additional block of text added before each new container definition.
    custom_derive_block: Option<String>,
    /// Whether definitions and fields should be marked as `pub`.
    track_visibility: bool,
}

/// Shared state for the code generation of a Rust source file.
struct RustEmitter<'a, T> {
    /// Writer.
    out: IndentedWriter<T>,
    /// Generator.
    generator: &'a CodeGenerator<'a>,
    /// Track which definitions have a known size. (Used to add `Box` types.)
    known_sizes: Cow<'a, HashSet<&'a str>>,
    /// Current namespace (e.g. vec!["my_package", "my_module", "MyClass"]).
    current_namespace: Vec<String>,
    /// When title-case is enabled, maps original full names to their
    /// `UpperCamelCase` equivalents so that `TypeName` references can be
    /// rewritten consistently.
    type_renames: HashMap<String, String>,
}

impl<'a> CodeGenerator<'a> {
    /// Create a Rust code generator for the given config.
    pub fn new(config: &'a CodeGeneratorConfig) -> Self {
        Self {
            config,
            derive_macros: vec!["Clone", "Debug", "PartialEq", "PartialOrd"]
                .into_iter()
                .map(String::from)
                .collect(),
            custom_derive_block: None,
            track_visibility: true,
        }
    }

    /// Which derive macros should be added (independently from serialization).
    pub fn with_derive_macros(mut self, derive_macros: Vec<String>) -> Self {
        self.derive_macros = derive_macros;
        self
    }

    /// Additional block of text added after `derive_macros` (if any), before
    /// each new container definition.
    pub fn with_custom_derive_block(mut self, custom_derive_block: Option<String>) -> Self {
        self.custom_derive_block = custom_derive_block;
        self
    }

    /// Whether definitions and fields should be marked as `pub`.
    pub fn with_track_visibility(mut self, track_visibility: bool) -> Self {
        self.track_visibility = track_visibility;
        self
    }

    /// Write container definitions in Rust.
    pub fn output(
        &self,
        out: &mut dyn Write,
        registry: &Registry,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let external_names: BTreeSet<String> = self
            .config
            .external_definitions
            .values()
            .cloned()
            .flatten()
            .collect();

        let known_sizes = external_names
            .iter()
            .map(<String as std::ops::Deref>::deref)
            .collect::<HashSet<_>>();

        let current_namespace = self
            .config
            .module_name
            .split('.')
            .map(String::from)
            .collect();

        // Build the title-case rename map when the option is enabled.
        let type_renames = if self.config.use_title_case {
            Self::build_rename_map(registry)
        } else {
            HashMap::new()
        };

        let mut emitter = RustEmitter {
            out: IndentedWriter::new(out, IndentConfig::Space(4)),
            generator: self,
            known_sizes: Cow::Owned(known_sizes),
            current_namespace,
            type_renames,
        };

        emitter.output_preamble()?;

        for ((ns, name), format) in registry {
            emitter.output_container(ns, name, format)?;

            // After emitting a struct, optionally emit builder and trait impls
            // inline — no post-processing string surgery needed.
            if let ContainerFormat::Struct(fields) = format {
                let struct_name = emitter.resolve_struct_name(ns, name);

                if self.config.generate_builders {
                    emitter.output_builder_impl(&struct_name, fields)?;
                }

                if self.config.generate_trait_impls {
                    emitter.output_trait_impls(&struct_name)?;
                }
            }

            emitter.known_sizes.to_mut().insert(name);
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Title-case helpers
    // ------------------------------------------------------------------

    /// Build a map from original full type names to their UpperCamelCase form.
    fn build_rename_map(registry: &Registry) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for (ns, name) in registry.keys() {
            let full_name = match ns {
                Some(n) => format!("{}_{}", n, name),
                None => name.clone(),
            };
            let title = full_name.to_upper_camel_case();
            if title != full_name {
                map.insert(full_name, title);
            }
        }
        map
    }
}

// =========================================================================
// RustEmitter
// =========================================================================

impl<'a, T> RustEmitter<'a, T>
where
    T: std::io::Write,
{
    // ------------------------------------------------------------------
    // Name conversion helpers
    // ------------------------------------------------------------------

    /// Apply the title-case rename map to a type name string.
    ///
    /// For simple names that are a direct key in the map this is a straight
    /// lookup.  For compound names (e.g. `Vec<Map<String, Vec<foo_bar>>>`)
    /// we replace all known substrings, longest-first to avoid partial
    /// matches.
    fn rename_type(&self, name: &str) -> String {
        if self.type_renames.is_empty() {
            return name.to_string();
        }
        // Fast path: direct lookup
        if let Some(renamed) = self.type_renames.get(name) {
            return renamed.clone();
        }
        // Slow path: replace all known names inside a compound expression.
        let mut result = name.to_string();
        let mut sorted: Vec<_> = self.type_renames.iter().collect();
        sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for (old, new) in sorted {
            result = result.replace(old.as_str(), new.as_str());
        }
        result
    }

    /// Compute the final struct name for a given registry entry, applying
    /// namespace prefixing and title-case conversion as appropriate.
    fn resolve_struct_name(&self, namespace: &Option<String>, name: &str) -> String {
        let raw = match namespace {
            Some(ns) => format!("{}_{}", ns, name),
            None => name.to_string(),
        };
        self.rename_type(&raw)
    }

    // ------------------------------------------------------------------
    // Output helpers
    // ------------------------------------------------------------------

    fn output_comment(&mut self, name: &str) -> std::io::Result<()> {
        let mut path = self.current_namespace.clone();
        path.push(name.to_string());
        if let Some(doc) = self.generator.config.comments.get(&path) {
            let text = textwrap::indent(doc, "/// ").replace("\n\n", "\n///\n");
            write!(self.out, "\n{}", text)?;
        }
        Ok(())
    }

    fn output_preamble(&mut self) -> Result<()> {
        // When a custom preamble is configured, use it verbatim instead of
        // the built-in default.
        if let Some(preamble) = &self.generator.config.custom_preamble {
            writeln!(self.out, "{}", preamble)?;
            writeln!(self.out)?;
            return Ok(());
        }

        let external_names = self
            .generator
            .config
            .external_definitions
            .values()
            .cloned()
            .flatten()
            .collect::<HashSet<_>>();
        writeln!(
            self.out,
            "#![allow(unused_imports, non_snake_case, non_camel_case_types, non_upper_case_globals)]"
        )?;
        if !external_names.contains("Map") {
            writeln!(self.out, "use std::collections::BTreeMap as Map;")?;
        }
        writeln!(self.out, "use serde::{{Serialize, Deserialize}};")?;
        if !external_names.contains("Bytes") {
            writeln!(self.out, "use serde_bytes::ByteBuf as Bytes;")?;
        }
        for (module, definitions) in &self.generator.config.external_definitions {
            if !module.is_empty() {
                writeln!(
                    self.out,
                    "use {}::{{{}}};",
                    module,
                    definitions.to_vec().join(", "),
                )?;
            }
        }
        writeln!(self.out)?;
        Ok(())
    }

    fn output_field_annotation(&mut self, format: &Format) -> std::io::Result<()> {
        use Format::*;
        match format {
            Str => writeln!(
                self.out,
                "#[serde(skip_serializing_if = \"String::is_empty\")]"
            )?,
            Option(_) => writeln!(
                self.out,
                "#[serde(skip_serializing_if = \"Option::is_none\")]"
            )?,
            Seq(_) => writeln!(
                self.out,
                "#[serde(skip_serializing_if = \"Vec::is_empty\")]"
            )?,
            _ => (),
        }
        Ok(())
    }

    fn quote_type(&self, format: &Format, known_sizes: Option<&HashSet<&str>>) -> String {
        use Format::*;
        match format {
            TypeName(x) => {
                let display_name = self.rename_type(x);
                if let Some(set) = known_sizes {
                    if !set.contains(x.as_str()) && !x.as_str().starts_with("Vec") {
                        return format!("Box<{}>", display_name);
                    }
                }
                display_name
            }
            Unit => "()".into(),
            Bool => "bool".into(),
            I8 => "i8".into(),
            I16 => "i16".into(),
            I32 => "i32".into(),
            I64 => "i64".into(),
            I128 => "i128".into(),
            U8 => "u8".into(),
            U16 => "u16".into(),
            U32 => "u32".into(),
            U64 => "u64".into(),
            U128 => "u128".into(),
            F32 => "f32".into(),
            F64 => "f64".into(),
            Char => "char".into(),
            Str => "String".into(),
            Bytes => "Bytes".into(),

            Option(format) => {
                format!("Option<{}>", self.quote_type(format, known_sizes))
            }
            Seq(format) => format!("Vec<{}>", self.quote_type(format, None)),
            Map { key, value } => format!(
                "Map<{}, {}>",
                self.quote_type(key, None),
                self.quote_type(value, None)
            ),
            Tuple(formats) => {
                format!("({})", self.quote_types(formats, known_sizes))
            }
            TupleArray { content, size } => {
                format!("[{}; {}]", self.quote_type(content, known_sizes), *size)
            }

            Variable(_) => panic!("unexpected value"),
        }
    }

    fn quote_types(&self, formats: &[Format], known_sizes: Option<&HashSet<&str>>) -> String {
        formats
            .iter()
            .map(|x| self.quote_type(x, known_sizes))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn output_fields(&mut self, base: &[&str], fields: &[Named<Format>]) -> Result<()> {
        // Do not add 'pub' within variants.
        let prefix = if base.len() <= 1 && self.generator.track_visibility {
            "pub "
        } else {
            ""
        };
        for field in fields {
            self.output_comment(&field.name)?;
            self.output_field_annotation(&field.value)?;
            writeln!(
                self.out,
                "{}{}: {},",
                prefix,
                field.name,
                self.quote_type(&field.value, Some(&self.known_sizes)),
            )?;
        }
        Ok(())
    }

    fn output_variant(&mut self, base: &str, name: &str, variant: &VariantFormat) -> Result<()> {
        self.output_comment(name)?;
        use VariantFormat::*;
        match variant {
            Unit => writeln!(self.out, "{},", name),
            NewType(format) => writeln!(
                self.out,
                "{}({}),",
                name,
                self.quote_type(format, Some(&self.known_sizes))
            ),
            Tuple(formats) => writeln!(
                self.out,
                "{}({}),",
                name,
                self.quote_types(formats, Some(&self.known_sizes))
            ),
            Struct(fields) => {
                writeln!(self.out, "{} {{", name)?;
                self.current_namespace.push(name.to_string());
                self.out.indent();
                self.output_fields(&[base, name], fields)?;
                self.out.unindent();
                self.current_namespace.pop();
                writeln!(self.out, "}},")
            }
            Variable(_) => panic!("incorrect value"),
        }
    }

    fn output_variants(
        &mut self,
        base: &str,
        variants: &BTreeMap<u32, Named<VariantFormat>>,
    ) -> Result<()> {
        for (expected_index, (index, variant)) in variants.iter().enumerate() {
            assert_eq!(*index, expected_index as u32);
            self.output_variant(base, &variant.name, &variant.value)?;
        }
        Ok(())
    }

    /// Returns `true` when every field in a struct is `Option<_>`.
    fn all_fields_optional(fields: &[Named<Format>]) -> bool {
        fields.iter().all(|f| matches!(f.value, Format::Option(_)))
    }

    fn output_container(
        &mut self,
        namespace: &Option<String>,
        name: &str,
        format: &ContainerFormat,
    ) -> Result<()> {
        self.output_comment(name)?;
        let mut derive_macros = self.generator.derive_macros.clone();
        derive_macros.push("Serialize".to_string());
        derive_macros.push("Deserialize".to_string());
        let mut prefix = String::new();

        use ContainerFormat::*;
        match format {
            UnitStruct => {
                if !derive_macros.is_empty() {
                    prefix.push_str(&format!("#[derive({})]\n", derive_macros.join(", ")));
                }
                if let Some(text) = &self.generator.custom_derive_block {
                    prefix.push_str(text);
                    prefix.push('\n');
                }
                writeln!(self.out, "{}struct {};\n", prefix, name)
            }
            NewTypeStruct(fmt) => {
                if !derive_macros.is_empty() {
                    prefix.push_str(&format!("#[derive({})]\n", derive_macros.join(", ")));
                }
                if let Some(text) = &self.generator.custom_derive_block {
                    prefix.push_str(text);
                    prefix.push('\n');
                }
                writeln!(
                    self.out,
                    "{}struct {}({}{});\n",
                    prefix,
                    name,
                    if self.generator.track_visibility {
                        "pub "
                    } else {
                        ""
                    },
                    self.quote_type(fmt, Some(&self.known_sizes))
                )
            }
            TupleStruct(formats) => {
                if !derive_macros.is_empty() {
                    prefix.push_str(&format!("#[derive({})]\n", derive_macros.join(", ")));
                }
                if let Some(text) = &self.generator.custom_derive_block {
                    prefix.push_str(text);
                    prefix.push('\n');
                }
                writeln!(
                    self.out,
                    "{}struct {}({});\n",
                    prefix,
                    name,
                    self.quote_types(formats, Some(&self.known_sizes))
                )
            }
            Struct(fields) => {
                // Smart Default: derive Default when every field is optional,
                // OR when the config forces it via `generate_default`.
                if Self::all_fields_optional(fields) || self.generator.config.generate_default {
                    derive_macros.push("Default".to_string());
                }

                prefix.clear();
                prefix.push_str(&format!("#[derive({})]\n", derive_macros.join(", ")));

                if let Some(text) = &self.generator.custom_derive_block {
                    prefix.push_str(text);
                    prefix.push('\n');
                }

                let mut struct_name = name.to_string();

                if let Some(ns) = namespace {
                    prefix.push_str(&format!("#[serde(rename = \"{}\")]\n", name));
                    struct_name = format!("{}_{}", ns, name);
                }

                // Apply title-case conversion to the struct name.
                let struct_name = self.rename_type(&struct_name);

                if self.generator.track_visibility {
                    prefix.push_str("pub ");
                }

                writeln!(self.out, "{}struct {} {{", prefix, struct_name)?;
                self.current_namespace.push(name.to_string());
                self.out.indent();
                self.output_fields(&[name], fields)?;
                self.out.unindent();
                self.current_namespace.pop();
                writeln!(self.out, "}}\n")
            }
            Enum(variants) => {
                if !derive_macros.is_empty() {
                    prefix.push_str(&format!("#[derive({})]\n", derive_macros.join(", ")));
                }
                if let Some(text) = &self.generator.custom_derive_block {
                    prefix.push_str(text);
                    prefix.push('\n');
                }
                if self.generator.track_visibility {
                    prefix.push_str("pub ");
                }

                writeln!(self.out, "{}enum {} {{", prefix, name)?;
                self.current_namespace.push(name.to_string());
                self.out.indent();
                self.output_variants(name, variants)?;
                self.out.unindent();
                self.current_namespace.pop();
                writeln!(self.out, "}}\n")
            }
        }
    }

    // ------------------------------------------------------------------
    // Inline builder generation
    // ------------------------------------------------------------------

    /// Emit an `impl StructName { pub fn new(…) -> Self { … } }` block for
    /// a struct that has at least one required (non-`Option`) field.
    ///
    /// When every field is optional, nothing is emitted (the struct already
    /// derives `Default`).
    fn output_builder_impl(&mut self, struct_name: &str, fields: &[Named<Format>]) -> Result<()> {
        let required_fields: Vec<_> = fields
            .iter()
            .filter(|f| !is_optional_format(&f.value))
            .collect();

        // Nothing to do when every field is already optional.
        if required_fields.is_empty() {
            return Ok(());
        }

        let use_title_case = self.generator.config.use_title_case;

        // Build the parameter list from required fields only.
        let params: Vec<String> = required_fields
            .iter()
            .map(|f| format!("{}: {}", f.name, format_to_type(&f.value, use_title_case)))
            .collect();

        writeln!(self.out, "impl {} {{", struct_name)?;
        writeln!(self.out, "    pub fn new({}) -> Self {{", params.join(", "))?;
        writeln!(self.out, "        Self {{")?;

        for field in fields {
            if is_optional_format(&field.value) {
                writeln!(
                    self.out,
                    "            {}: {},",
                    field.name,
                    default_value_for(&field.value)
                )?;
            } else {
                writeln!(self.out, "            {},", field.name)?;
            }
        }

        writeln!(self.out, "        }}")?;
        writeln!(self.out, "    }}")?;
        writeln!(self.out, "}}\n")?;

        Ok(())
    }

    // ------------------------------------------------------------------
    // Inline trait impl generation
    // ------------------------------------------------------------------

    /// Emit `TerraJson` and `TerraResource` trait implementations for
    /// `struct_name` if it appears in the config's `resource_meta`.
    fn output_trait_impls(&mut self, struct_name: &str) -> Result<()> {
        let meta = &self.generator.config.resource_meta;

        // Find the matching meta entry.  The struct_name we receive is the
        // final (possibly title-cased) name, so compare against the
        // (also possibly renamed) meta struct_name.
        let matching = meta.iter().find(|m| {
            let meta_name = if self.generator.config.use_title_case {
                m.struct_name.to_upper_camel_case()
            } else {
                m.struct_name.clone()
            };
            meta_name == struct_name
        });

        let m = match matching {
            Some(m) => m,
            None => return Ok(()),
        };

        let provider_const = provider_source_to_const(&m.provider_source);

        // TerraJson impl
        writeln!(
            self.out,
            "impl crate::terra::TerraJson for {} {{",
            struct_name
        )?;
        writeln!(self.out, "    fn to_json(&self) -> serde_json::Value {{")?;
        writeln!(
            self.out,
            "        serde_json::to_value(self).expect(\"serialization should not fail\")"
        )?;
        writeln!(self.out, "    }}")?;
        writeln!(self.out, "}}\n")?;

        // TerraResource impl
        writeln!(
            self.out,
            "impl crate::terra::TerraResource for {} {{",
            struct_name
        )?;
        writeln!(
            self.out,
            "    fn resource_type(&self) -> &'static str {{ \"{}\" }}",
            m.resource_type
        )?;
        writeln!(
            self.out,
            "    fn provider(&self) -> &'static crate::terra::TerraProvider {{ &crate::terra::TerraProvider::{} }}",
            provider_const
        )?;
        writeln!(self.out, "}}\n")?;

        Ok(())
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
/// This is used by the builder generator and intentionally mirrors the
/// `quote_type` logic but operates without an emitter reference.
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
