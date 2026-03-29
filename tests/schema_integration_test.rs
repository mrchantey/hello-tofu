//! Integration test: generate bindings from the full AWS + Cloudflare schema.json
//!
//! This test verifies that the binding generator can process a real-world,
//! full-size provider schema without panicking and that the generated code
//! has the expected structure.

use hello_tofu::binding_generator::BindingGenerator;

#[test]
fn test_generate_bindings_from_real_schema() {
    // Read the full schema
    let schema =
        BindingGenerator::read_schema("./schema.json").expect("Failed to read schema.json");

    let generator = BindingGenerator::new().with_builders(true);

    // Generate bindings to a string
    let code = generator
        .generate_to_string(&schema)
        .expect("Failed to generate bindings from schema.json");

    // Basic sanity checks on generated code
    assert!(!code.is_empty(), "Generated code should not be empty");
    assert!(
        code.contains("pub struct config"),
        "Generated code should contain a config struct"
    );
    assert!(
        code.contains("resource_root"),
        "Generated code should contain resource_root enum"
    );
    assert!(
        code.contains("aws_lightsail_instance_details"),
        "Generated code should contain aws_lightsail_instance_details"
    );
    assert!(
        code.contains("aws_lambda_function_details"),
        "Generated code should contain aws_lambda_function_details"
    );

    // Check that the code contains the builder impls
    assert!(
        code.contains("impl aws_lightsail_instance_details"),
        "Generated code should contain builder for aws_lightsail_instance_details"
    );

    // Check that both providers generated types
    assert!(code.contains("aws_"), "Should contain AWS resource types");
    assert!(
        code.contains("cloudflare_"),
        "Should contain Cloudflare resource types"
    );

    // Check that we have a meaningful amount of generated code
    let lines: Vec<&str> = code.lines().collect();
    assert!(
        lines.len() > 1000,
        "Expected >1000 lines of generated code, got {}",
        lines.len()
    );
}

/// Verify the generated output contains the expected structure by checking
/// the first and last few lines rather than reading the massive output fully.
#[test]
fn test_generated_code_structure() {
    let schema =
        BindingGenerator::read_schema("./schema.json").expect("Failed to read schema.json");

    let generator = BindingGenerator::new();
    let code = generator
        .generate_to_string(&schema)
        .expect("Failed to generate bindings");

    let lines: Vec<&str> = code.lines().collect();

    // Check the preamble (first ~5 lines)
    assert!(
        lines[0].contains("allow(unused_imports"),
        "First line should be the allow attribute, got: {}",
        lines[0]
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("use serde::{Serialize, Deserialize}")),
        "Should have serde imports"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("use std::collections::BTreeMap as Map")),
        "Should have Map import"
    );

    // Check that we have a meaningful number of lines (the full schema generates thousands)
    assert!(
        lines.len() > 1000,
        "Expected >1000 lines of generated code, got {}",
        lines.len()
    );

    // Check that both providers generated types
    assert!(code.contains("aws_"), "Should contain AWS resource types");
    // The cloudflare provider name is "registry.opentofu.org/cloudflare/cloudflare"
    // After split('/').last() it becomes "cloudflare" as the provider name
    assert!(
        code.contains("cloudflare_"),
        "Should contain Cloudflare resource types"
    );
}
