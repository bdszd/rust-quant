use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::modules::backtest::{
    application::{
        ports::{BacktestRepository, OutboxRepository, PendingOutboxEvent},
        queries::BacktestTaskView,
    },
    domain::{BacktestJob, BacktestTask, StrategyConfig, TaskStatus},
};

pub struct PgBacktestRepository {
    pool: PgPool,
}

impl PgBacktestRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BacktestRepository for PgBacktestRepository {
    async fn create_task(
        &self,
        task: &BacktestTask,
        job: &BacktestJob,
    ) -> Result<(), String> {
        let config_json = serde_json::to_string(&job.config)
            .map_err(|error| format!("序列化回测配置失败：{error}"))?;
        let event_id = Uuid::new_v4();
        let payload = serde_json::json!({
            "event_id": event_id,
            "task_id": task.id,
            "job_id": job.id,
        });
        let payload_json = serde_json::to_string(&payload)
            .map_err(|error| format!("序列化 Outbox 消息失败：{error}"))?;

        let mut transaction = self.pool.begin().await
            .map_err(|error| format!("开启创建回测任务事务失败：{error}"))?;

        sqlx::query(
            r#"
            INSERT INTO backtest_tasks (id, user_id, status)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(task.id)
        .bind(task.user_id)
        .bind(task.status.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("保存回测任务失败：{error}"))?;

        sqlx::query(
            r#"
            INSERT INTO backtest_jobs (id, task_id, config)
            VALUES ($1, $2, $3::jsonb)
            "#,
        )
        .bind(job.id)
        .bind(job.task_id)
        .bind(config_json)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("保存回测执行配置失败：{error}"))?;

        sqlx::query(
            r#"
            INSERT INTO backtest_outbox (id, task_id, event_type, payload)
            VALUES ($1, $2, 'backtest.requested', $3::jsonb)
            "#,
        )
        .bind(event_id)
        .bind(task.id)
        .bind(payload_json)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("保存回测 Outbox 事件失败：{error}"))?;

        transaction.commit().await
            .map_err(|error| format!("提交创建回测任务事务失败：{error}"))?;

        Ok(())
    }

    async fn find_task(&self, task_id: Uuid) -> Result<Option<BacktestTaskView>, String> {
        let row = sqlx::query(
            r#"
            SELECT
                t.id,
                t.status,
                j.config::text AS config_json,
                r.payload::text AS result_json,
                latest_attempt.error,
                latest_attempt.duration_ms AS execution_duration_ms
            FROM backtest_tasks t
            JOIN backtest_jobs j ON j.task_id = t.id
            LEFT JOIN backtest_results r ON r.task_id = t.id
            LEFT JOIN LATERAL (
                SELECT error, duration_ms
                FROM backtest_task_attempts
                WHERE task_id = t.id
                ORDER BY attempt_number DESC
                LIMIT 1
            ) latest_attempt ON TRUE
            WHERE t.id = $1
            "#,
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| format!("查询回测任务失败：{error}"))?;

        row.map(map_task_view).transpose()
    }

    async fn list_tasks(&self, limit: i64) -> Result<Vec<BacktestTaskView>, String> {
        let rows = sqlx::query(
            r#"
            SELECT
                t.id,
                t.status,
                j.config::text AS config_json,
                r.payload::text AS result_json,
                latest_attempt.error,
                latest_attempt.duration_ms AS execution_duration_ms
            FROM backtest_tasks t
            JOIN backtest_jobs j ON j.task_id = t.id
            LEFT JOIN backtest_results r ON r.task_id = t.id
            LEFT JOIN LATERAL (
                SELECT error, duration_ms
                FROM backtest_task_attempts
                WHERE task_id = t.id
                ORDER BY attempt_number DESC
                LIMIT 1
            ) latest_attempt ON TRUE
            ORDER BY t.created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| format!("查询回测历史失败：{error}"))?;

        rows.into_iter().map(map_task_view).collect()
    }
}

fn map_task_view(row: sqlx::postgres::PgRow) -> Result<BacktestTaskView, String> {
    let id: Uuid = row.try_get("id")
        .map_err(|error| format!("读取任务 ID 失败：{error}"))?;
    let status: String = row.try_get("status")
        .map_err(|error| format!("读取任务状态失败：{error}"))?;
    let config_json: String = row.try_get("config_json")
        .map_err(|error| format!("读取任务配置失败：{error}"))?;
    let result_json: Option<String> = row.try_get("result_json")
        .map_err(|error| format!("读取回测结果失败：{error}"))?;
    let error: Option<String> = row.try_get("error")
        .map_err(|error| format!("读取任务错误信息失败：{error}"))?;
    let execution_duration_ms: Option<i64> = row.try_get("execution_duration_ms")
        .map_err(|error| format!("读取执行耗时失败：{error}"))?;

    let config: crate::modules::backtest::domain::BacktestJobConfig =
        serde_json::from_str(&config_json)
            .map_err(|error| format!("解析回测配置失败：{error}"))?;
    let result: Option<Value> = result_json
        .map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|error| format!("解析回测结果失败：{error}"))?;

    let status = match status.as_str() {
        "queued" => TaskStatus::Queued,
        "running" => TaskStatus::Running,
        "succeeded" => TaskStatus::Succeeded,
        "failed" => TaskStatus::Failed,
        other => return Err(format!("数据库中存在未知任务状态：{other}")),
    };

    let (strategy, short_window, long_window) = match config.strategy {
        StrategyConfig::SmaCross { short_window, long_window } => {
            ("sma_cross".to_string(), Some(short_window), Some(long_window))
        }
        StrategyConfig::BuyAndHold => ("buy_and_hold".to_string(), None, None),
    };

    Ok(BacktestTaskView {
        id,
        dataset: config.dataset,
        strategy,
        short_window,
        long_window,
        status,
        result,
        error,
        execution_duration_ms,
    })
}


#[async_trait]
impl OutboxRepository for PgBacktestRepository {
    async fn claim_pending(
        &self,
        publisher_id: &str,
        limit: i64,
        lease_seconds: i64,
    ) -> Result<Vec<PendingOutboxEvent>, String> {
        let rows = sqlx::query(
            r#"
            WITH candidates AS (
                SELECT id
                FROM backtest_outbox
                WHERE published_at IS NULL
                  AND (locked_until IS NULL OR locked_until < NOW())
                ORDER BY created_at
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            UPDATE backtest_outbox AS outbox
            SET locked_by = $2,
                locked_until = NOW() + ($3 * INTERVAL '1 second'),
                publish_attempts = publish_attempts + 1
            FROM candidates
            WHERE outbox.id = candidates.id
            RETURNING
                outbox.id,
                outbox.task_id,
                outbox.event_type,
                outbox.payload::text AS payload_json
            "#,
        )
        .bind(limit.clamp(1, 100))
        .bind(publisher_id)
        .bind(lease_seconds.clamp(1, 300))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| format!("领取待发布 Outbox 事件失败：{error}"))?;

        rows.into_iter()
            .map(|row| {
                let id: Uuid = row.try_get("id")
                    .map_err(|error| format!("读取 Outbox ID 失败：{error}"))?;
                let task_id: Uuid = row.try_get("task_id")
                    .map_err(|error| format!("读取 Outbox task_id 失败：{error}"))?;
                let event_type: String = row.try_get("event_type")
                    .map_err(|error| format!("读取 Outbox event_type 失败：{error}"))?;
                let payload_json: String = row.try_get("payload_json")
                    .map_err(|error| format!("读取 Outbox payload 失败：{error}"))?;
                let payload = serde_json::from_str(&payload_json)
                    .map_err(|error| format!("解析 Outbox payload 失败：{error}"))?;

                Ok(PendingOutboxEvent {
                    id,
                    task_id,
                    event_type,
                    payload,
                })
            })
            .collect()
    }

    async fn mark_published(
        &self,
        event_id: Uuid,
        publisher_id: &str,
    ) -> Result<bool, String> {
        let result = sqlx::query(
            r#"
            UPDATE backtest_outbox
            SET published_at = NOW(),
                locked_by = NULL,
                locked_until = NULL,
                last_error = NULL
            WHERE id = $1
              AND locked_by = $2
              AND published_at IS NULL
            "#,
        )
        .bind(event_id)
        .bind(publisher_id)
        .execute(&self.pool)
        .await
        .map_err(|error| format!("标记 Outbox 事件已发布失败：{error}"))?;

        Ok(result.rows_affected() == 1)
    }

    async fn release_after_failure(
        &self,
        event_id: Uuid,
        publisher_id: &str,
        error: &str,
    ) -> Result<(), String> {
        sqlx::query(
            r#"
            UPDATE backtest_outbox
            SET locked_by = NULL,
                locked_until = NULL,
                last_error = $3
            WHERE id = $1
              AND locked_by = $2
              AND published_at IS NULL
            "#,
        )
        .bind(event_id)
        .bind(publisher_id)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(|error| format!("释放 Outbox 事件领取锁失败：{error}"))?;

        Ok(())
    }
}
