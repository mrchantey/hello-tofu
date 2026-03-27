import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------
// const appName = "hello-lightsail";
const appName = pulumi.getProject();
const config = new pulumi.Config();
const stage = pulumi.getStack(); // e.g. "prod"
const prefix = `${appName}--${stage}`;

const availabilityZone = config.require("availabilityZone");
const blueprintId = config.require("blueprintId");
const bundleId = config.require("bundleId");
const serverPort = config.requireNumber("serverPort");

// ---------------------------------------------------------------------------
// Key Pair  –  auto-generated so we can SSH into the instance later
// ---------------------------------------------------------------------------
const keyPair = new aws.lightsail.KeyPair(`${prefix}--keypair`, {
	namePrefix: `${prefix}--keypair`,
});

// ---------------------------------------------------------------------------
// Static IP  –  survives instance replacement
// ---------------------------------------------------------------------------
const staticIp = new aws.lightsail.StaticIp(`${prefix}--ip`, {
	name: `${prefix}--ip`,
});

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------
const userData = `#!/bin/bash
set -euo pipefail

# Create app directory
mkdir -p /opt/${appName}

# Create a systemd service so the server starts on boot and restarts on crash
cat > /etc/systemd/system/${appName}.service <<'EOF'
[Unit]
Description=Hello Lightsail HTTP Server
After=network.target

[Service]
Type=simple
ExecStart=/opt/${appName}/app
WorkingDirectory=/opt/${appName}
Restart=always
RestartSec=3
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable ${appName}.service
# The service will fail until the binary is deployed, that's expected.
`;

const instance = new aws.lightsail.Instance(`${prefix}--instance`, {
	name: `${prefix}--instance`,
	availabilityZone,
	blueprintId,
	bundleId,
	keyPairName: keyPair.name,
	userData,
	tags: {
		Project: appName,
		Stage: stage,
	},
});

// ---------------------------------------------------------------------------
// Attach Static IP → Instance
// ---------------------------------------------------------------------------
const _ipAttachment = new aws.lightsail.StaticIpAttachment(
	`${prefix}--ip-attach`,
	{
		instanceName: instance.name,
		staticIpName: staticIp.name,
	},
);

// ---------------------------------------------------------------------------
// Firewall – open the server port and SSH
// ---------------------------------------------------------------------------
const _publicPorts = new aws.lightsail.InstancePublicPorts(
	`${prefix}--ports`,
	{
		instanceName: instance.name,
		portInfos: [
			{
				protocol: "tcp",
				fromPort: serverPort,
				toPort: serverPort,
			},
			{
				protocol: "tcp",
				fromPort: 22,
				toPort: 22,
			},
		],
	},
);

// ---------------------------------------------------------------------------
// Exports  –  consumed by the justfile for deploy / ssh helpers
// ---------------------------------------------------------------------------
export const instanceName = instance.name;
export const staticIpAddress = staticIp.ipAddress;
export const privateKey = pulumi.secret(keyPair.privateKey);
export const publicKey = keyPair.publicKey;
export const port = serverPort;
