//! Auto-generated Terraform provider bindings — do not edit by hand.

#![allow(unused_imports, non_snake_case, non_camel_case_types, non_upper_case_globals)]
use std::collections::BTreeMap as Map;
use serde::{Serialize, Deserialize};
use serde_json;
#[derive(Clone, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CloudflareDnsRecordDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_modified_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Map<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub for_each: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_on: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxiable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Map<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags_modified_on: Option<String>,
    pub ttl: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub r#type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub zone_id: String,
}

impl CloudflareDnsRecordDetails {
    pub fn new(name: String, ttl: i64, r#type: String, zone_id: String) -> Self {
        Self {
            comment: None,
            comment_modified_on: None,
            content: None,
            count: None,
            created_on: None,
            data: None,
            depends_on: None,
            for_each: None,
            id: None,
            meta: None,
            modified_on: None,
            name,
            priority: None,
            provider: None,
            proxiable: None,
            proxied: None,
            settings: None,
            tags: None,
            tags_modified_on: None,
            ttl,
            r#type,
            zone_id,
        }
    }
}

impl crate::terra::TerraJson for CloudflareDnsRecordDetails {
    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serialization should not fail")
    }
}

impl crate::terra::TerraResource for CloudflareDnsRecordDetails {
    fn resource_type(&self) -> &'static str { "cloudflare_dns_record" }
    fn provider(&self) -> &'static crate::terra::TerraProvider { &crate::terra::TerraProvider::CLOUDFLARE }
}

