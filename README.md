# hello-tofu

Typesafe Terraform/OpenTofu infrastructure in pure Rust.

Generate typed bindings from provider schemas, declare your infrastructure with proper Rust types, and export valid Terraform JSON — with full compile-time safety.

## Quick Start

### 1. Generate provider bindings

The project includes a pre-exported `schema.json` (gitignored). To regenerate it:

```sh
tofu init
tofu providers schema -json > schema.json
```

Then generate typed Rust modules from the schema:

```sh
cargo run --example generate
```

This reads `schema.json` and writes filtered, `UpperCamelCase` provider bindings to `src/providers/`.

### 2. Use typed resources in your config

```rust
use hello_tofu::config_exporter::ConfigExporter;
use hello_tofu::providers::aws_lambda::*;

let bucket = AwsS3BucketDetails {
    bucket: Some("my-assets".into()),
    force_destroy: Some(true),
    ..Default::default()
};

let func = AwsLambdaFunctionDetails::new(
    "my-function".into(),
    "${aws_iam_role.role.arn}".into(),
);

let exporter = ConfigExporter::new()
    .with_resource("assets", &bucket)
    .with_resource("router", &func);

exporter.export_to_file("main.tf.json")?;
```

Providers are registered automatically from the resources you add — no manual `add_required_provider` calls needed.

### 3. Validate

Validation is built into the exporter:

```rust
// Export and run tofu init + validate in one call
exporter.export_and_validate("out/main.tf.json")?;
```

### 4. Run the examples

```sh
# Lambda + API Gateway + Cloudflare DNS
cargo run --example lambda --features providers_aws_lambda,providers_cloudflare_dns

# Lightsail instance with static IP
cargo run --example lightsail --features providers_aws_lightsail
```

## Architecture

```
src/
├── terra.rs              # Core types: TerraProvider, TerraResource, TerraJson, ResourceFilter
├── binding_generator.rs  # High-level schema → Rust code generator
├── config_generator.rs   # Roundtrip orchestrator: tofu init → schema → codegen
├── config_exporter.rs    # Typed config builder → Terraform JSON
├── schema_bindgen/       # Low-level schema parsing and code emission
│   ├── binding.rs        #   Schema deserialization + serde-reflection registry
│   ├── emit.rs           #   Rust code emitter (smart Default, TitleCase)
│   └── config.rs         #   Code generator configuration
└── providers/            # Generated provider modules (feature-gated)
    ├── aws_lambda.rs     #   S3, IAM, Lambda, API Gateway, etc.
    ├── aws_lightsail.rs  #   Lightsail instance, key pair, static IP, etc.
    └── cloudflare_dns.rs #   Cloudflare DNS records
```

### Key traits

| Trait | Purpose |
|-------|---------|
| `TerraJson` | Serialize a value to Terraform-compatible JSON |
| `TerraResource` | Marks a struct as a Terraform resource with a type and provider |
| `TerraProvider` | Identifies a provider (AWS, Cloudflare, etc.) with source and version |

### Binding generation pipeline

1. **Filter** — `ResourceFilter` selects only the resources you need from the massive provider schema
2. **Parse** — Schema JSON is deserialized into a serde-reflection `Registry`
3. **Emit** — The `CodeGenerator` writes Rust structs with:
   - `UpperCamelCase` type names (via `heck`)
   - Smart `#[derive(Default)]` — only when all fields are optional
   - `new()` constructors for structs with required fields
   - `#[serde(skip_serializing_if)]` annotations for clean JSON output
4. **Trait impls** — `TerraResource` and `TerraJson` implementations are appended automatically

### Features

| Feature | Description |
|---------|-------------|
| `providers_aws_lambda` | Lambda, API Gateway, S3, IAM resources |
| `providers_aws_lightsail` | Lightsail instance, key pair, static IP, ports |
| `providers_cloudflare_dns` | Cloudflare DNS record |

## License

MIT