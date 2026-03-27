//! Export Terraform/OpenTofu configurations as JSON.
//!
//! The [`ConfigExporter`] collects provider configurations, resources, data sources,
//! and variable/output/local definitions, then serializes them into valid
//! Terraform JSON configuration.

use serde::Serialize;
use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::Path;

/// A required provider declaration.
pub struct ProviderRequirement {
    /// The provider source, e.g. "hashicorp/aws"
    pub source: String,
    /// The version constraint, e.g. "~> 5.0"
    pub version: String,
}

/// A Terraform variable definition.
pub struct Variable {
    pub r#type: Option<String>,
    pub default: Option<Value>,
    pub description: Option<String>,
}

/// A Terraform output definition.
pub struct Output {
    pub value: Value,
    pub description: Option<String>,
    pub sensitive: Option<bool>,
}

/// Builds and exports a complete Terraform JSON configuration.
///
/// # Example
/// ```rust,ignore
/// let mut exporter = ConfigExporter::new();
/// exporter.add_required_provider("aws", "hashicorp/aws", "~> 5.0");
/// exporter.add_provider("aws", &serde_json::json!({"region": "us-west-2"}));
/// exporter.add_resource("aws_instance", "web", &my_instance);
/// exporter.export_to_file("main.tf.json")?;
/// ```
pub struct ConfigExporter {
    required_providers: Map<String, Value>,
    providers: Map<String, Value>,
    resources: Map<String, Value>,
    data_sources: Map<String, Value>,
    variables: Map<String, Value>,
    outputs: Map<String, Value>,
    locals: Map<String, Value>,
}

impl ConfigExporter {
    /// Create a new empty configuration exporter.
    pub fn new() -> Self {
        Self {
            required_providers: Map::new(),
            providers: Map::new(),
            resources: Map::new(),
            data_sources: Map::new(),
            variables: Map::new(),
            outputs: Map::new(),
            locals: Map::new(),
        }
    }

    /// Add a required provider declaration.
    ///
    /// # Arguments
    /// * `name` - Local name for the provider, e.g. "aws"
    /// * `source` - Provider source, e.g. "hashicorp/aws"
    /// * `version` - Version constraint, e.g. "~> 5.0"
    pub fn add_required_provider(&mut self, name: &str, source: &str, version: &str) -> &mut Self {
        self.required_providers.insert(
            name.to_string(),
            json!({
                "source": source,
                "version": version,
            }),
        );
        self
    }

    /// Add a provider configuration block.
    ///
    /// `config` can be any Serialize type — typically one of the generated
    /// provider detail structs, or a raw `serde_json::Value`.
    pub fn add_provider(
        &mut self,
        name: &str,
        config: &impl Serialize,
    ) -> Result<&mut Self, serde_json::Error> {
        let value = serde_json::to_value(config)?;
        self.providers.insert(name.to_string(), value);
        Ok(self)
    }

    /// Add a resource block.
    ///
    /// # Arguments
    /// * `resource_type` - The Terraform resource type, e.g. "aws_instance"
    /// * `name` - The local name for this resource instance, e.g. "web_server"
    /// * `config` - The resource configuration (any Serialize type)
    pub fn add_resource(
        &mut self,
        resource_type: &str,
        name: &str,
        config: &impl Serialize,
    ) -> Result<&mut Self, serde_json::Error> {
        let value = serde_json::to_value(config)?;
        let type_map = self
            .resources
            .entry(resource_type.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = type_map {
            map.insert(name.to_string(), value);
        }
        Ok(self)
    }

    /// Add a data source block.
    pub fn add_data_source(
        &mut self,
        data_type: &str,
        name: &str,
        config: &impl Serialize,
    ) -> Result<&mut Self, serde_json::Error> {
        let value = serde_json::to_value(config)?;
        let type_map = self
            .data_sources
            .entry(data_type.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = type_map {
            map.insert(name.to_string(), value);
        }
        Ok(self)
    }

    /// Add a variable definition.
    pub fn add_variable(&mut self, name: &str, variable: Variable) -> &mut Self {
        let mut var_obj = Map::new();
        if let Some(t) = variable.r#type {
            var_obj.insert("type".to_string(), Value::String(t));
        }
        if let Some(d) = variable.default {
            var_obj.insert("default".to_string(), d);
        }
        if let Some(desc) = variable.description {
            var_obj.insert("description".to_string(), Value::String(desc));
        }
        self.variables
            .insert(name.to_string(), Value::Object(var_obj));
        self
    }

    /// Add an output definition.
    pub fn add_output(&mut self, name: &str, output: Output) -> &mut Self {
        let mut out_obj = Map::new();
        out_obj.insert("value".to_string(), output.value);
        if let Some(desc) = output.description {
            out_obj.insert("description".to_string(), Value::String(desc));
        }
        if let Some(sensitive) = output.sensitive {
            out_obj.insert("sensitive".to_string(), Value::Bool(sensitive));
        }
        self.outputs
            .insert(name.to_string(), Value::Object(out_obj));
        self
    }

    /// Add a local value.
    pub fn add_local(
        &mut self,
        name: &str,
        value: impl Serialize,
    ) -> Result<&mut Self, serde_json::Error> {
        let v = serde_json::to_value(value)?;
        self.locals.insert(name.to_string(), v);
        Ok(self)
    }

    /// Build the complete Terraform JSON configuration.
    pub fn to_value(&self) -> Value {
        let mut root = Map::new();

        // terraform block with required_providers
        if !self.required_providers.is_empty() {
            root.insert(
                "terraform".to_string(),
                json!({
                    "required_providers": Value::Object(self.required_providers.clone()),
                }),
            );
        }

        // provider block
        if !self.providers.is_empty() {
            root.insert(
                "provider".to_string(),
                Value::Object(self.providers.clone()),
            );
        }

        // variable block
        if !self.variables.is_empty() {
            root.insert(
                "variable".to_string(),
                Value::Object(self.variables.clone()),
            );
        }

        // locals block
        if !self.locals.is_empty() {
            root.insert("locals".to_string(), Value::Object(self.locals.clone()));
        }

        // resource block
        if !self.resources.is_empty() {
            root.insert(
                "resource".to_string(),
                Value::Object(self.resources.clone()),
            );
        }

        // data block
        if !self.data_sources.is_empty() {
            root.insert("data".to_string(), Value::Object(self.data_sources.clone()));
        }

        // output block
        if !self.outputs.is_empty() {
            root.insert("output".to_string(), Value::Object(self.outputs.clone()));
        }

        Value::Object(root)
    }

    /// Serialize to a pretty-printed JSON string.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.to_value())
    }

    /// Write the configuration to a writer.
    pub fn export_to_writer(
        &self,
        writer: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let json = self.to_json_pretty()?;
        writer.write_all(json.as_bytes())?;
        writer.write_all(b"\n")?;
        Ok(())
    }

    /// Write the configuration to a file.
    pub fn export_to_file(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = std::fs::File::create(path)?;
        self.export_to_writer(&mut file)
    }
}

impl Default for ConfigExporter {
    fn default() -> Self {
        Self::new()
    }
}
