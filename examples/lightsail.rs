use hello_tofu::config_exporter::{ConfigExporter, Output, Variable};
use serde_json::json;
use std::env;
use std::fs;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_name = "hello-lightsail";
    let stage = "dev";
    let prefix = format!("{}--{}", app_name, stage);

    let mut exporter = ConfigExporter::new();

    // Required providers
    exporter.add_required_provider("aws", "hashicorp/aws", "~> 6.0");

    // Provider config
    exporter.add_provider(
        "aws",
        &json!({
            "region": "us-east-1"
        }),
    )?;

    // Variables
    exporter.add_variable(
        "availability_zone",
        Variable {
            r#type: Some("string".to_string()),
            default: Some(json!("us-east-1a")),
            description: Some("Lightsail availability zone".to_string()),
        },
    );
    exporter.add_variable(
        "blueprint_id",
        Variable {
            r#type: Some("string".to_string()),
            default: Some(json!("amazon_linux_2023")),
            description: Some("Lightsail instance blueprint".to_string()),
        },
    );
    exporter.add_variable(
        "bundle_id",
        Variable {
            r#type: Some("string".to_string()),
            default: Some(json!("nano_3_0")),
            description: Some("Lightsail instance bundle".to_string()),
        },
    );
    exporter.add_variable(
        "server_port",
        Variable {
            r#type: Some("number".to_string()),
            default: Some(json!(8080)),
            description: Some("The port the server listens on".to_string()),
        },
    );

    // Key Pair
    exporter.add_resource(
        "aws_lightsail_key_pair",
        "keypair",
        &json!({
            "name_prefix": format!("{}--keypair", prefix),
        }),
    )?;

    // Static IP
    exporter.add_resource(
        "aws_lightsail_static_ip",
        "ip",
        &json!({
            "name": format!("{}--ip", prefix),
        }),
    )?;

    // Instance
    let user_data = format!(
        r#"#!/bin/bash
set -euo pipefail
mkdir -p /opt/{app_name}
cat > /etc/systemd/system/{app_name}.service <<'EOF'
[Unit]
Description=Hello Lightsail HTTP Server
After=network.target
[Service]
Type=simple
ExecStart=/opt/{app_name}/app
WorkingDirectory=/opt/{app_name}
Restart=always
RestartSec=3
Environment=RUST_LOG=info
[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable {app_name}.service
"#,
        app_name = app_name
    );

    exporter.add_resource(
        "aws_lightsail_instance",
        "instance",
        &json!({
            "name": format!("{}--instance", prefix),
            "availability_zone": "${var.availability_zone}",
            "blueprint_id": "${var.blueprint_id}",
            "bundle_id": "${var.bundle_id}",
            "key_pair_name": "${aws_lightsail_key_pair.keypair.name}",
            "user_data": user_data,
            "tags": {
                "Project": app_name,
                "Stage": stage
            }
        }),
    )?;

    // Static IP Attachment
    exporter.add_resource(
        "aws_lightsail_static_ip_attachment",
        "ip_attach",
        &json!({
            "instance_name": "${aws_lightsail_instance.instance.name}",
            "static_ip_name": "${aws_lightsail_static_ip.ip.name}",
        }),
    )?;

    // Instance Public Ports
    exporter.add_resource(
        "aws_lightsail_instance_public_ports",
        "ports",
        &json!({
            "instance_name": "${aws_lightsail_instance.instance.name}",
            "port_info": [
                {
                    "protocol": "tcp",
                    "from_port": "${var.server_port}",
                    "to_port": "${var.server_port}"
                },
                {
                    "protocol": "tcp",
                    "from_port": 22,
                    "to_port": 22
                }
            ]
        }),
    )?;

    // Outputs
    exporter.add_output(
        "instance_name",
        Output {
            value: json!("${aws_lightsail_instance.instance.name}"),
            description: Some("The Lightsail instance name".to_string()),
            sensitive: None,
        },
    );
    exporter.add_output(
        "static_ip_address",
        Output {
            value: json!("${aws_lightsail_static_ip.ip.ip_address}"),
            description: Some("The static IP address".to_string()),
            sensitive: None,
        },
    );

    // Write to a temp directory and validate
    let out_dir = env::temp_dir().join("hello-tofu-lightsail");
    fs::create_dir_all(&out_dir)?;

    let out_path = out_dir.join("main.tf.json");
    exporter.export_to_file(&out_path)?;
    println!("Generated: {}", out_path.display());

    // Print the first 50 and last 20 lines
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
