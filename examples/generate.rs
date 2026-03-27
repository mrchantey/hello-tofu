//! Generate typed Terraform provider bindings from schema.json.
//!
//! Run with:
//!     cargo run --example generate
//!
//! This reads the existing `schema.json` (produced by `tofu providers schema -json`)
//! and generates filtered, TitleCase Rust modules under `src/providers/`.

use hello_tofu::config_generator::{ConfigGenerator, TerraProviderGenerator};
use hello_tofu::terra::TerraProvider;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let generator = ConfigGenerator::default()
        // AWS resources used by examples/lambda.rs
        .with_resources(
            TerraProviderGenerator::new(TerraProvider::AWS, "src/providers/aws_lambda.rs"),
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
        )
        // AWS resources used by examples/lightsail.rs
        .with_resources(
            TerraProviderGenerator::new(TerraProvider::AWS, "src/providers/aws_lightsail.rs"),
            [
                "aws_lightsail_instance",
                "aws_lightsail_instance_public_ports",
                "aws_lightsail_key_pair",
                "aws_lightsail_static_ip",
                "aws_lightsail_static_ip_attachment",
            ],
        )
        // Cloudflare resources used by examples/lambda.rs
        .with_resources(
            TerraProviderGenerator::new(
                TerraProvider::CLOUDFLARE,
                "src/providers/cloudflare_dns.rs",
            ),
            ["cloudflare_dns_record"],
        );

    // Use the existing schema.json instead of running the full tofu init cycle.
    eprintln!("Generating provider bindings from schema.json ...");
    generator.generate_from_schema("schema.json")?;
    eprintln!("Done! Provider modules written to src/providers/");

    Ok(())
}
