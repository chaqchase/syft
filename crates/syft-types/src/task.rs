use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{EntityId, now_utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: EntityId,
    pub repo_id: EntityId,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub constraints: Vec<String>,
    pub labels: Vec<String>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum TaskStatus {
    #[default]
    Open,
    InReview,
    Done,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum TaskPriority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub author: Author,
    pub model: Option<ModelInfo>,
    pub prompt_ref: Option<EntityId>,
    pub retrieved_context_refs: Vec<EntityId>,
    pub tool_run_refs: Vec<EntityId>,
    pub session_ref: Option<EntityId>,
    pub created_at: DateTime<Utc>,
}

impl Default for Provenance {
    fn default() -> Self {
        Self {
            author: Author::Human {
                user_id: "unknown".to_string(),
            },
            model: None,
            prompt_ref: None,
            retrieved_context_refs: Vec::new(),
            tool_run_refs: Vec::new(),
            session_ref: None,
            created_at: now_utc(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Author {
    Human { user_id: String },
    Agent { agent_id: String },
    Tool { tool_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub provider: String,
    pub model_name: String,
    pub temperature: Option<f32>,
}
