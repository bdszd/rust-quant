use serde_json::Value;
use uuid::Uuid;

use super::super::domain::TaskStatus;

/// HTTP 和 Worker 之外使用的任务查询模型。
///
/// 这是应用层的只读结果，不是数据库行，也不是 Axum response DTO。
#[derive(Debug, Clone)]
pub struct BacktestTaskView {
    pub id: Uuid,
    pub dataset: String,
    pub strategy: String,
    pub short_window: Option<usize>,
    pub long_window: Option<usize>,
    pub status: TaskStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub execution_duration_ms: Option<i64>,
}
