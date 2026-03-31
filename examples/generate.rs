//! Generate typed Terraform provider bindings from schema.json.
//!
//! Run with:
//!     cargo run --example generate --features bindings_generator
//!
//! This reads the existing `schema.json` (produced by `tofu providers schema -json`)
//! and generates filtered, TitleCase Rust modules under `src/providers/`.

use hello_tofu::schema_binding_generator::{BindingFile, SchemaBindingGenerator};
use hello_tofu::terra::TerraProvider;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let generator = SchemaBindingGenerator::default()
        // AWS resources used by examples/lambda.rs
        .with_file(
            BindingFile::new("src/providers/aws_lambda.rs").with_resources(
                TerraProvider::AWS,
                [
                    "aws_api_gateway_rest_api",
                    "aws_apigatewayv2_api",
                    "aws_apigatewayv2_integration",
                    "aws_apigatewayv2_route",
                    "aws_apigatewayv2_stage",
                    "aws_iam_role",
                    "aws_iam_role_policy_attachment",
                    "aws_lambda_function",
                    "aws_lambda_function_url",
                    "aws_lambda_permission",
                    "aws_s3_bucket",
                ],
            ),
        )
        // AWS resources used by examples/lightsail.rs
        .with_file(
            BindingFile::new("src/providers/aws_lightsail.rs").with_resources(
                TerraProvider::AWS,
                [
                    "aws_lightsail_instance",
                    "aws_lightsail_instance_public_ports",
                    "aws_lightsail_key_pair",
                    "aws_lightsail_static_ip",
                    "aws_lightsail_static_ip_attachment",
                ],
            ),
        )
        // Cloudflare resources used by examples/lambda.rs
        .with_file(
            BindingFile::new("src/providers/cloudflare_dns.rs")
                .with_resources(TerraProvider::CLOUDFLARE, ["cloudflare_dns_record"]),
        );

    // Use the existing schema.json instead of running the full tofu init cycle.
    eprintln!("Generating provider bindings from schema.json ...");
    generator.generate()?;
    eprintln!("Done! Provider modules written to src/providers/");

    Ok(())
}
