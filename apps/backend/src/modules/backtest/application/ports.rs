use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use super::queries::BacktestTaskView;
use crate::modules::backtest::domain::{
    BacktestJob, BacktestTask,
    attempt::BacktestAttempt,
    job::BacktestJobConfig,
};

/// 应用层需要的持久化能力。
///
/// PostgreSQL 是它的一种实现；应用层不依赖 SQLx。
#[async_trait]
pub trait BacktestRepository: Send + Sync {
    async fn create_task(&self, task: &BacktestTask, job: &BacktestJob) -> Result<(), String>;

    async fn find_task(&self, task_id: Uuid) -> Result<Option<BacktestTaskView>, String>;

    async fn list_tasks(&self, limit: i64) -> Result<Vec<BacktestTaskView>, String>;
}

/// Publisher 从持久化队列中领取的事件。
#[derive(Debug, Clone)]
pub struct PendingOutboxEvent {
    pub id: Uuid,
    pub task_id: Uuid,
    pub event_type: String,
    pub payload: Value,
}

/// Outbox 的持久化能力。
///
/// 使用带期限的领取锁，避免多个 Publisher 同时处理同一条待发送事件。
#[async_trait]
pub trait OutboxRepository: Send + Sync {
    async fn claim_pending(
        &self,
        publisher_id: &str,
        limit: i64,
        lease_seconds: i64,
    ) -> Result<Vec<PendingOutboxEvent>, String>;

    async fn mark_published(&self, event_id: Uuid, publisher_id: &str) -> Result<bool, String>;

    async fn release_after_failure(
        &self,
        event_id: Uuid,
        publisher_id: &str,
        error: &str,
    ) -> Result<(), String>;
}

/// Worker 从应用层领取到的一次具体执行。
#[derive(Debug, Clone)]
pub struct ClaimedBacktestJob {
    pub task_id: Uuid,
    pub attempt: BacktestAttempt,
    pub config: BacktestJobConfig,
}

#[derive(Debug)]
pub enum ClaimExecutionOutcome {
    Claimed(ClaimedBacktestJob),
    AlreadyFinished,
    LeaseHeld,
}

/// 回测执行所需的持久化能力。
///
/// Worker 只依赖这个端口，不直接依赖 PostgreSQL Repository。
#[async_trait]
pub trait BacktestExecutionRepository: Send + Sync {
    async fn claim_execution(
        &self,
        task_id: Uuid,
        job_id: Uuid,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<ClaimExecutionOutcome, String>;

    async fn heartbeat_attempt(
        &self,
        attempt_id: Uuid,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<bool, String>;

    async fn complete_success(
        &self,
        attempt_id: Uuid,
        task_id: Uuid,
        worker_id: &str,
        payload: &Value,
        duration_ms: i64,
    ) -> Result<(), String>;

    async fn complete_failure(
        &self,
        attempt_id: Uuid,
        task_id: Uuid,
        worker_id: &str,
        message: &str,
        duration_ms: Option<i64>,
    ) -> Result<(), String>;
}
