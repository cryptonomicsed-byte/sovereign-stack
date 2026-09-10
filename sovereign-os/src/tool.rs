//! Tool registry — capability-gated tool invocation.
//! Every tool is a named async function with a JSON schema and capability requirement.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name:        String,
    pub description: String,
    pub input_schema: Value,
    pub required_capability: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool:    String,
    pub success: bool,
    pub output:  Value,
    pub error:   Option<String>,
}

impl ToolResult {
    pub fn ok(tool: &str, output: Value) -> Self {
        Self { tool: tool.into(), success: true, output, error: None }
    }
    pub fn err(tool: &str, e: impl Into<String>) -> Self {
        Self { tool: tool.into(), success: false, output: Value::Null, error: Some(e.into()) }
    }
}

/// The tool registry — maps names to definitions.
/// Execution is decoupled: the registry stores metadata; callers invoke
/// tools through their own dispatch logic.
#[derive(Debug, Default, Clone)]
pub struct ToolRegistry {
    tools: Arc<std::sync::RwLock<HashMap<String, ToolDef>>>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&self, def: ToolDef) {
        self.tools.write().unwrap().insert(def.name.clone(), def);
    }

    pub fn get(&self, name: &str) -> Option<ToolDef> {
        self.tools.read().unwrap().get(name).cloned()
    }

    pub fn list(&self) -> Vec<ToolDef> {
        self.tools.read().unwrap().values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.tools.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
}
