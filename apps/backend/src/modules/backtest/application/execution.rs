use std::sync::Arc;

use serde_json::Value;
use uuid::Uuid;

use super::ports::{BacktestExecutionRepository, ClaimExecutionOutcome};

/// 回测执行应用服务。
///
/// 它为 Worker 提供任务认领、续租和结果提交用例，
/// 把 Worker 与具体数据库实现隔离开。
pub struct BacktestExecutionService {
    repository: Arc<dyn BacktestExecutionRepository>,
}

impl BacktestExecutionService {
    pub fn new(repository: Arc<dyn BacktestExecutionRepository>) -> Self {
        Self { repository }
    }

    pub async fn claim(
        &self,
        task_id: Uuid,
        job_id: Uuid,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<ClaimExecutionOutcome, String> {
        self.repository
            .claim_execution(task_id, job_id, worker_id, lease_seconds)
            .await
    }

    pub async fn heartbeat(
        &self,
        attempt_id: Uuid,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<bool, String> {
        self.repository
            .heartbeat_attempt(attempt_id, worker_id, lease_seconds)
            .await
    }

    pub async fn complete_success(
        &self,
        attempt_id: Uuid,
        task_id: Uuid,
        worker_id: &str,
        payload: &Value,
        duration_ms: i64,
    ) -> Result<(), String> {
        self.repository
            .complete_success(attempt_id, task_id, worker_id, payload, duration_ms)
            .await
    }

    pub async fn complete_failure(
        &self,
        attempt_id: Uuid,
        task_id: Uuid,
        worker_id: &str,
        message: &str,
        duration_ms: Option<i64>,
    ) -> Result<(), String> {
        self.repository
            .complete_failure(attempt_id, task_id, worker_id, message, duration_ms)
            .await
    }
}
