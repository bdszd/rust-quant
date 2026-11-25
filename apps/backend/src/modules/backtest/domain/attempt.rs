use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl AttemptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// 一次实际执行。一个 Task 可以有多次 Attempt。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestAttempt {
    pub id: Uuid,
    pub task_id: Uuid,
    pub attempt_number: i32,
    pub status: AttemptStatus,
    pub error: Option<String>,
}
