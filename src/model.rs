use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ENGINE_VERSION: &str = concat!(
    "rowtide/",
    env!("CARGO_PKG_VERSION"),
    "/checks-1/",
    env!("ROWTIDE_IMPLEMENTATION_SHA256")
);
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub task: String,
    pub purpose: String,
    #[serde(default)]
    pub contract: Option<String>,
    pub inputs: Vec<Input>,
    #[serde(default)]
    pub checks: Vec<Check>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub declarations: Vec<Declaration>,
    #[serde(default)]
    pub budgets: Budgets,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub id: String,
    pub path: String,
    pub role: String,
    pub format: String,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub reader: Option<Reader>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reader {
    pub path: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub input: String,
    pub kind: CheckKind,
    #[serde(default)]
    pub scope: Option<Filter>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CheckKind {
    Schema {
        required: BTreeMap<String, String>,
    },
    Key {
        columns: Vec<String>,
    },
    Coverage {
        columns: Vec<String>,
        expected_input: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub column: String,
    pub min: Option<String>,
    pub max: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub pointer: Option<String>,
    #[serde(default)]
    pub excerpt: Option<String>,
    pub origin: String,
    pub scope: String,
    #[serde(default)]
    pub input_sha256: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub id: String,
    pub input: String,
    pub description: String,
    pub evidence: Vec<String>,
    pub expected: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Budgets {
    pub hash_bytes: u64,
    pub timeout_seconds: u64,
    pub memory_mb: u64,
    pub temp_mb: u64,
    pub max_examples: usize,
}
impl Default for Budgets {
    fn default() -> Self {
        Self {
            hash_bytes: 2_147_483_648,
            timeout_seconds: 60,
            memory_mb: 512,
            temp_mb: 512,
            max_examples: 5,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    pub id: String,
    pub status: String,
    pub origin: String,
    pub reason: String,
    pub details: serde_json::Value,
    #[serde(default)]
    pub reused: bool,
}
impl Finding {
    pub fn unknown(id: &str, reason: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: "unknown".into(),
            origin: "measured".into(),
            reason: reason.into(),
            details: serde_json::Value::Null,
            reused: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub path: String,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
    pub modified_ns: Option<u128>,
    pub state: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub engine_version: String,
    pub operation: String,
    pub execution: String,
    pub request: Request,
    pub bindings: BTreeMap<String, Binding>,
    pub findings: Vec<Finding>,
    pub freshness: BTreeMap<String, String>,
    pub consumption: Vec<Finding>,
    pub next_actions: Vec<String>,
    pub limitations: Vec<String>,
    pub previous: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActualReceipt {
    pub source: String,
    pub mappings: Vec<ConsumptionMapping>,
    #[serde(default)]
    pub notes: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumptionMapping {
    pub input: String,
    pub path_pointer: String,
    pub sha256_pointer: Option<String>,
    pub columns_pointer: Option<String>,
    pub role_pointer: Option<String>,
    pub reader_sha256_pointer: Option<String>,
    pub reader_parameters_pointer: Option<String>,
}
