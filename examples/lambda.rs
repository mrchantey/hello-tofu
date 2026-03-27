use hello_tofu::config_exporter::{ConfigExporter, Output};
use serde_json::json;
use std::env;
use std::fs;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_name = "beet-site";
    let stage = "dev";
    let prefix = format!("{}--{}", app_name, stage);

    let mut exporter = ConfigExporter::new();

    // Required providers
    exporter.add_required_provider("aws", "hashicorp/aws", "~> 6.0");

    // Provider config
    exporter.add_provider(
        "aws",
        &json!({
            "region": "us-west-2"
        }),
    )?;

    // S3 Buckets
    exporter.add_resource(
        "aws_s3_bucket",
        "assets",
        &json!({
            "bucket": format!("{}--assets", prefix),
            "force_destroy": true
        }),
    )?;

    exporter.add_resource(
        "aws_s3_bucket",
        "html",
        &json!({
            "bucket": format!("{}--html", prefix),
            "force_destroy": true
        }),
    )?;

    // IAM Role for Lambda
    let assume_role_policy = json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Action": "sts:AssumeRole",
            "Effect": "Allow",
            "Principal": {
                "Service": "lambda.amazonaws.com"
            }
        }]
    });

    exporter.add_resource(
        "aws_iam_role",
        "lambda_role",
        &json!({
            "name": format!("{}--lambda-role", prefix),
            "assume_role_policy": assume_role_policy.to_string()
        }),
    )?;

    // IAM Policy Attachments
    exporter.add_resource(
        "aws_iam_role_policy_attachment",
        "lambda_basic",
        &json!({
            "role": "${aws_iam_role.lambda_role.name}",
            "policy_arn": "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
        }),
    )?;

    // Lambda function
    exporter.add_resource(
        "aws_lambda_function",
        "router",
        &json!({
            "function_name": format!("{}--router", prefix),
            "role": "${aws_iam_role.lambda_role.arn}",
            "runtime": "provided.al2023",
            "handler": "bootstrap",
            "filename": "lambda.zip",
            "timeout": 180,
            "memory_size": 1024,
            "source_code_hash": ""
        }),
    )?;

    // Lambda Function URL
    exporter.add_resource(
        "aws_lambda_function_url",
        "router_url",
        &json!({
            "function_name": "${aws_lambda_function.router.function_name}",
            "authorization_type": "NONE"
        }),
    )?;

    // API Gateway v2
    exporter.add_resource(
        "aws_apigatewayv2_api",
        "gateway",
        &json!({
            "name": format!("{}--gateway", prefix),
            "protocol_type": "HTTP"
        }),
    )?;

    // API Gateway Integration
    exporter.add_resource(
        "aws_apigatewayv2_integration",
        "lambda_integration",
        &json!({
            "api_id": "${aws_apigatewayv2_api.gateway.id}",
            "integration_type": "AWS_PROXY",
            "integration_uri": "${aws_lambda_function.router.invoke_arn}",
            "payload_format_version": "2.0"
        }),
    )?;

    // API Gateway Route (default)
    exporter.add_resource(
        "aws_apigatewayv2_route",
        "default_route",
        &json!({
            "api_id": "${aws_apigatewayv2_api.gateway.id}",
            "route_key": "$default",
            "target": "integrations/${aws_apigatewayv2_integration.lambda_integration.id}"
        }),
    )?;

    // API Gateway Stage
    exporter.add_resource(
        "aws_apigatewayv2_stage",
        "default_stage",
        &json!({
            "api_id": "${aws_apigatewayv2_api.gateway.id}",
            "name": "$default",
            "auto_deploy": true
        }),
    )?;

    // Lambda Permission for API Gateway
    exporter.add_resource(
        "aws_lambda_permission",
        "apigw_lambda",
        &json!({
            "action": "lambda:InvokeFunction",
            "function_name": "${aws_lambda_function.router.function_name}",
            "principal": "apigateway.amazonaws.com",
            "source_arn": "${aws_apigatewayv2_api.gateway.execution_arn}/*/*"
        }),
    )?;

    // Outputs
    exporter.add_output(
        "api_endpoint",
        Output {
            value: json!("${aws_apigatewayv2_api.gateway.api_endpoint}"),
            description: Some("The API Gateway endpoint URL".to_string()),
            sensitive: None,
        },
    );
    exporter.add_output(
        "function_url",
        Output {
            value: json!("${aws_lambda_function_url.router_url.function_url}"),
            description: Some("The Lambda function URL".to_string()),
            sensitive: None,
        },
    );
    exporter.add_output(
        "assets_bucket",
        Output {
            value: json!("${aws_s3_bucket.assets.bucket}"),
            description: Some("The S3 assets bucket name".to_string()),
            sensitive: None,
        },
    );

    // Write to a temp directory and validate
    let out_dir = env::temp_dir().join("hello-tofu-lambda");
    fs::create_dir_all(&out_dir)?;

    let out_path = out_dir.join("main.tf.json");
    exporter.export_to_file(&out_path)?;
    println!("Generated: {}", out_path.display());

    // Print first 50 and last 20 lines
    let content = fs::read_to_string(&out_path)?;
    let lines: Vec<&str> = content.lines().collect();
    println!("\n--- First 50 lines ---");
    for line in lines.iter().take(50) {
        println!("{}", line);
    }
    if lines.len() > 70 {
        println!("\n... ({} lines total) ...\n", lines.len());
        println!("--- Last 20 lines ---");
        for line in lines.iter().skip(lines.len() - 20) {
            println!("{}", line);
        }
    }

    // Run tofu validate
    println!("\n--- Running tofu init + validate ---");
    let init = Command::new("tofu")
        .current_dir(&out_dir)
        .args(["init"])
        .output()?;
    if !init.status.success() {
        eprintln!(
            "tofu init stderr: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        std::process::exit(1);
    }
    println!("tofu init: OK");

    let validate = Command::new("tofu")
        .current_dir(&out_dir)
        .args(["validate", "-json"])
        .output()?;

    let validate_output = String::from_utf8_lossy(&validate.stdout);
    println!("tofu validate output: {}", validate_output);

    if !validate.status.success() {
        eprintln!(
            "tofu validate stderr: {}",
            String::from_utf8_lossy(&validate.stderr)
        );
        std::process::exit(1);
    }
    println!("tofu validate: PASSED!");

    Ok(())
}
