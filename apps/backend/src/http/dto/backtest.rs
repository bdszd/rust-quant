use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::modules::backtest::{
    application::queries::BacktestTaskView,
    domain::{BacktestTask, TaskStatus},
};

/// HTTP 返回格式与既有 backtests API 保持一致。
#[derive(Debug, Serialize)]
pub struct BacktestTaskResponse {
    pub id: String,
    pub dataset: String,
    pub strategy: String,
    pub short_window: Option<usize>,
    pub long_window: Option<usize>,
    pub status: TaskStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub execution_duration_ms: Option<i64>,
}

impl BacktestTaskResponse {
    pub fn queued(
        task: &BacktestTask,
        dataset: String,
        strategy: String,
        short_window: Option<usize>,
        long_window: Option<usize>,
    ) -> Self {
        Self {
            id: task.id.to_string(),
            dataset,
            strategy,
            short_window,
            long_window,
            status: task.status,
            result: None,
            error: None,
            execution_duration_ms: None,
        }
    }

    pub fn from_view(view: BacktestTaskView) -> Self {
        Self {
            id: view.id.to_string(),
            dataset: view.dataset,
            strategy: view.strategy,
            short_window: view.short_window,
            long_window: view.long_window,
            status: view.status,
            result: view.result,
            error: view.error,
            execution_duration_ms: view.execution_duration_ms,
        }
    }

    pub fn parse_id(id: &str) -> Result<Uuid, uuid::Error> {
        Uuid::parse_str(id)
    }
}
