//! Roundtrip schema + binding generator.
//!
//! [`SchemaBindingGenerator`] orchestrates the full workflow:
//!
//! 1. Write a `providers.tf.json` declaring the required providers.
//! 2. Run `tofu init` to download provider plugins.
//! 3. Run `tofu providers schema -json` to export the full schema.
//! 4. Parse the schema with [`BindingGenerator`] (applying filters).
//! 5. Write the generated Rust files to the specified output paths.

use crate::binding_generator::BindingGenerator;
use crate::terra::TerraProvider;
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// ProviderBindingTarget — per-provider output configuration
// ---------------------------------------------------------------------------

/// Pairs a [`TerraProvider`] with the output path for its generated bindings.
///
/// This is a configuration struct — it does not generate anything on its own.
/// Pass it to [`SchemaBindingGenerator::with_resources`] to register which
/// provider resources should be generated and where the output should be
/// written.
pub struct ProviderBindingTarget {
    /// The provider to generate bindings for.
    pub provider: TerraProvider,
    /// Destination file path (relative to the crate root), e.g.
    /// `"src/providers/aws_lambda.rs"`.
    pub path: PathBuf,
}

impl ProviderBindingTarget {
    pub fn new(provider: TerraProvider, path: impl Into<PathBuf>) -> Self {
        Self {
            provider,
            path: path.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// SchemaBindingGenerator
// ---------------------------------------------------------------------------

/// Orchestrates the full roundtrip: providers → tofu init → schema → codegen.
///
/// Holds a [`BindingGenerator`] that can be customised before generation.
/// The binding generator's [`CodeGeneratorConfig`] controls all code-generation
/// options (title case, builders, trait impls, preamble, etc.).
///
/// # Example
///
/// ```rust,ignore
/// SchemaBindingGenerator::default()
///     .with_resources(
///         ProviderBindingTarget::new(TerraProvider::AWS, "src/providers/aws_lambda.rs"),
///         ["aws_lambda_function", "aws_s3_bucket"],
///     )
///     .with_resources(
///         ProviderBindingTarget::new(TerraProvider::CLOUDFLARE, "src/providers/cloudflare_dns.rs"),
///         ["cloudflare_dns_record"],
///     )
///     .generate()?;
/// ```
pub struct SchemaBindingGenerator {
    /// Each entry maps a provider binding target to its list of resource type names.
    targets: Vec<(ProviderBindingTarget, Vec<String>)>,
    /// Working directory for tofu operations.  Defaults to
    /// `target/terra-bindings-generator`.
    work_dir: PathBuf,
    /// The binding generator used for each target.  Users can pre-configure
    /// this to control code-generation options; per-target filter and preamble
    /// are applied automatically on top.
    binding_generator: BindingGenerator,
}

impl Default for SchemaBindingGenerator {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            work_dir: PathBuf::from("target/terra-bindings-generator"),
            binding_generator: BindingGenerator::new()
                .with_title_case(true)
                .with_builders(true)
                .with_trait_impls(true)
                .with_custom_preamble(build_preamble()),
        }
    }
}

impl SchemaBindingGenerator {
    /// Add a provider and its resource list.
    pub fn with_resources(
        mut self,
        target: ProviderBindingTarget,
        resources: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let resources: Vec<String> = resources.into_iter().map(Into::into).collect();
        self.targets.push((target, resources));
        self
    }

    /// Override the working directory used for `tofu init` / schema export.
    pub fn with_work_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.work_dir = dir.into();
        self
    }

    /// Replace the [`BindingGenerator`] used for code generation.
    ///
    /// The filter and custom preamble are still set per-target automatically;
    /// everything else (title case, builders, trait impls, etc.) comes from
    /// the generator you supply here.
    pub fn with_binding_generator(mut self, generator: BindingGenerator) -> Self {
        self.binding_generator = generator;
        self
    }

    /// Return a shared reference to the current [`BindingGenerator`].
    pub fn binding_generator(&self) -> &BindingGenerator {
        &self.binding_generator
    }

    /// Return a mutable reference to the current [`BindingGenerator`].
    pub fn binding_generator_mut(&mut self) -> &mut BindingGenerator {
        &mut self.binding_generator
    }

    /// Run the full generation pipeline.
    pub fn generate(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Prepare the working directory.
        self.prepare_work_dir()?;

        // 2. Write providers.tf.json
        self.write_providers_tf()?;

        // 3. tofu init
        self.run_tofu_init()?;

        // 4. tofu providers schema -json > schema.json
        let schema_path = self.run_tofu_schema()?;

        // 5. For each provider target, generate bindings with appropriate filter.
        self.generate_bindings(&schema_path)?;

        Ok(())
    }

    /// Like [`generate`](Self::generate) but skip steps 1–4 and use an
    /// existing `schema.json` file directly.  Useful when the schema has
    /// already been exported (saves the slow `tofu init` step).
    pub fn generate_from_schema(
        &self,
        schema_path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.generate_bindings(schema_path.as_ref())
    }

    // ------------------------------------------------------------------
    // Internal steps
    // ------------------------------------------------------------------

    fn prepare_work_dir(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.work_dir.exists() {
            std::fs::remove_dir_all(&self.work_dir)?;
        }
        std::fs::create_dir_all(&self.work_dir)?;
        Ok(())
    }

    fn write_providers_tf(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut required_providers = serde_json::Map::new();

        for (target, _) in &self.targets {
            let p = &target.provider;
            // Deduplicate by local name.
            let local = p.local_name().to_string();
            if required_providers.contains_key(&local) {
                continue;
            }
            required_providers.insert(
                local,
                json!({
                    "source": p.short_source(),
                    "version": p.version.as_ref(),
                }),
            );
        }

        let tf_json = json!({
            "terraform": {
                "required_providers": required_providers,
            }
        });

        let path = self.work_dir.join("providers.tf.json");
        let mut f = std::fs::File::create(&path)?;
        serde_json::to_writer_pretty(&mut f, &tf_json)?;
        f.write_all(b"\n")?;

        eprintln!("[schema_binding_generator] wrote {}", path.display());
        Ok(())
    }

    fn run_tofu_init(&self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!(
            "[schema_binding_generator] running tofu init in {}",
            self.work_dir.display()
        );
        let output = Command::new("tofu")
            .current_dir(&self.work_dir)
            .args(["init"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("tofu init failed:\n{}", stderr).into());
        }
        eprintln!("[schema_binding_generator] tofu init: OK");
        Ok(())
    }

    fn run_tofu_schema(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let schema_path = self.work_dir.join("schema.json");
        eprintln!(
            "[schema_binding_generator] running tofu providers schema → {}",
            schema_path.display()
        );

        let output = Command::new("tofu")
            .current_dir(&self.work_dir)
            .args(["providers", "schema", "-json"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("tofu providers schema failed:\n{}", stderr).into());
        }

        std::fs::write(&schema_path, &output.stdout)?;
        eprintln!(
            "[schema_binding_generator] schema exported ({:.1} MB)",
            output.stdout.len() as f64 / 1_048_576.0
        );
        Ok(schema_path)
    }

    fn generate_bindings(&self, schema_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let schema = BindingGenerator::read_schema(schema_path)?;

        for (target, resources) in &self.targets {
            let filter = crate::terra::BindingFilter::default()
                .with_resources(target.provider.source.as_ref(), resources.clone());

            // Clone the base binding generator and apply the per-target filter.
            let binding_gen = self.binding_generator.clone().with_filter(filter);

            // Ensure the parent directory exists.
            if let Some(parent) = target.path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            binding_gen.generate_to_file(&schema, &target.path)?;
            eprintln!("[schema_binding_generator] wrote {}", target.path.display());
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Standard preamble for generated provider modules.
fn build_preamble() -> String {
    [
        "//! Auto-generated Terraform provider bindings — do not edit by hand.",
        "",
        "#![allow(unused_imports, non_snake_case, non_camel_case_types, non_upper_case_globals)]",
        "use std::collections::BTreeMap as Map;",
        "use serde::{Serialize, Deserialize};",
        "use serde_json;",
    ]
    .join("\n")
}
