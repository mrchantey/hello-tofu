//! Roundtrip binding generator.
//!
//! [`ConfigGenerator`] orchestrates the full workflow:
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
// TerraProviderGenerator — per-provider output configuration
// ---------------------------------------------------------------------------

/// Pairs a [`TerraProvider`] with the output path for its generated bindings.
pub struct TerraProviderGenerator {
    /// The provider to generate bindings for.
    pub provider: TerraProvider,
    /// Destination file path (relative to the crate root), e.g.
    /// `"src/providers/aws_lambda.rs"`.
    pub path: PathBuf,
}

impl TerraProviderGenerator {
    pub fn new(provider: TerraProvider, path: impl Into<PathBuf>) -> Self {
        Self {
            provider,
            path: path.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// ConfigGenerator
// ---------------------------------------------------------------------------

/// Orchestrates the full roundtrip: providers → tofu init → schema → codegen.
///
/// # Example
///
/// ```rust,ignore
/// ConfigGenerator::default()
///     .with_resources(
///         TerraProviderGenerator::new(TerraProvider::AWS, "src/providers/aws_lambda.rs"),
///         ["aws_lambda_function", "aws_s3_bucket"],
///     )
///     .with_resources(
///         TerraProviderGenerator::new(TerraProvider::CLOUDFLARE, "src/providers/cloudflare_dns.rs"),
///         ["cloudflare_dns_record"],
///     )
///     .generate()?;
/// ```
pub struct ConfigGenerator {
    /// Each entry maps a provider generator to its list of resource type names.
    targets: Vec<(TerraProviderGenerator, Vec<String>)>,
    /// Working directory for tofu operations.  Defaults to
    /// `target/terra-bindings-generator`.
    work_dir: PathBuf,
}

impl Default for ConfigGenerator {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            work_dir: PathBuf::from("target/terra-bindings-generator"),
        }
    }
}

impl ConfigGenerator {
    /// Add a provider and its resource list.
    pub fn with_resources(
        mut self,
        generator: TerraProviderGenerator,
        resources: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let resources: Vec<String> = resources.into_iter().map(Into::into).collect();
        self.targets.push((generator, resources));
        self
    }

    /// Override the working directory used for `tofu init` / schema export.
    pub fn with_work_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.work_dir = dir.into();
        self
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

        for (pg, _) in &self.targets {
            let p = &pg.provider;
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

        eprintln!("[config_generator] wrote {}", path.display());
        Ok(())
    }

    fn run_tofu_init(&self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!(
            "[config_generator] running tofu init in {}",
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
        eprintln!("[config_generator] tofu init: OK");
        Ok(())
    }

    fn run_tofu_schema(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let schema_path = self.work_dir.join("schema.json");
        eprintln!(
            "[config_generator] running tofu providers schema → {}",
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
            "[config_generator] schema exported ({:.1} MB)",
            output.stdout.len() as f64 / 1_048_576.0
        );
        Ok(schema_path)
    }

    fn generate_bindings(&self, schema_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let schema = BindingGenerator::read_schema(schema_path)?;

        // Group targets by provider source so we can build a combined filter
        // per output file.  Usually there's one provider per file.
        //
        // target_index → (output_path, filter, provider_source)
        // We iterate targets in order and generate each one.
        for (pg, resources) in &self.targets {
            let filter = crate::terra::ResourceFilter::default()
                .with_resources(pg.provider.source.as_ref(), resources.clone());

            let preamble = build_preamble();

            let binding_gen = BindingGenerator::new()
                .with_filter(filter)
                .with_title_case(true)
                .with_builders(true)
                .with_trait_impls(true)
                .with_custom_preamble(preamble);

            // Ensure the parent directory exists.
            if let Some(parent) = pg.path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            binding_gen.generate_to_file(&schema, &pg.path)?;
            eprintln!("[config_generator] wrote {}", pg.path.display());
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
