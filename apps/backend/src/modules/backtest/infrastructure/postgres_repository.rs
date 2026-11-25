use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::modules::backtest::{
    application::{
        ports::{
            BacktestExecutionRepository, BacktestRepository, ClaimExecutionOutcome,
            ClaimedBacktestJob, OutboxRepository, PendingOutboxEvent,
        },
        queries::BacktestTaskView,
    },
    domain::{
        BacktestJob, BacktestTask, StrategyConfig, TaskStatus,
        attempt::{AttemptStatus, BacktestAttempt},
        result::BacktestResultRecord,
    },
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
    async fn create_task(&self, task: &BacktestTask, job: &BacktestJob) -> Result<(), String> {
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

        let mut transaction = self
            .pool
            .begin()
            .await
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

        transaction
            .commit()
            .await
            .map_err(|error| format!("提交创建回测任务事务失败：{error}"))?;

        Ok(())
    }

    async fn find_task(&self, task_id: Uuid, user_id: Uuid) -> Result<Option<BacktestTaskView>, String> {
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
            WHERE t.id = $1 AND t.user_id = $2
            "#,
        )
        .bind(task_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| format!("查询回测任务失败：{error}"))?;

        row.map(map_task_view).transpose()
    }

    async fn list_tasks(&self, user_id: Uuid, limit: i64) -> Result<Vec<BacktestTaskView>, String> {
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
            WHERE t.user_id = $1
            ORDER BY t.created_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| format!("查询回测历史失败：{error}"))?;

        rows.into_iter().map(map_task_view).collect()
    }
}

fn map_task_view(row: sqlx::postgres::PgRow) -> Result<BacktestTaskView, String> {
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| format!("读取任务 ID 失败：{error}"))?;
    let status: String = row
        .try_get("status")
        .map_err(|error| format!("读取任务状态失败：{error}"))?;
    let config_json: String = row
        .try_get("config_json")
        .map_err(|error| format!("读取任务配置失败：{error}"))?;
    let result_json: Option<String> = row
        .try_get("result_json")
        .map_err(|error| format!("读取回测结果失败：{error}"))?;
    let error: Option<String> = row
        .try_get("error")
        .map_err(|error| format!("读取任务错误信息失败：{error}"))?;
    let execution_duration_ms: Option<i64> = row
        .try_get("execution_duration_ms")
        .map_err(|error| format!("读取执行耗时失败：{error}"))?;

    let config: crate::modules::backtest::domain::BacktestJobConfig =
        serde_json::from_str(&config_json).map_err(|error| format!("解析回测配置失败：{error}"))?;
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
        StrategyConfig::SmaCross {
            short_window,
            long_window,
        } => (
            "sma_cross".to_string(),
            Some(short_window),
            Some(long_window),
        ),
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
                let id: Uuid = row
                    .try_get("id")
                    .map_err(|error| format!("读取 Outbox ID 失败：{error}"))?;
                let task_id: Uuid = row
                    .try_get("task_id")
                    .map_err(|error| format!("读取 Outbox task_id 失败：{error}"))?;
                let event_type: String = row
                    .try_get("event_type")
                    .map_err(|error| format!("读取 Outbox event_type 失败：{error}"))?;
                let payload_json: String = row
                    .try_get("payload_json")
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

    async fn mark_published(&self, event_id: Uuid, publisher_id: &str) -> Result<bool, String> {
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

impl PgBacktestRepository {
    /// 原子认领任务并创建一次 Attempt。
    ///
    /// queued 任务可以首次认领；running 任务只有在上一次租约过期后才允许接管。
    pub async fn claim_execution(
        &self,
        task_id: Uuid,
        job_id: Uuid,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<ClaimExecutionOutcome, String> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| format!("开启任务认领事务失败：{error}"))?;

        let task_row = sqlx::query("SELECT status FROM backtest_tasks WHERE id = $1 FOR UPDATE")
            .bind(task_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| format!("锁定回测任务失败：{error}"))?;

        let Some(task_row) = task_row else {
            tx.commit()
                .await
                .map_err(|error| format!("提交空认领事务失败：{error}"))?;
            return Ok(ClaimExecutionOutcome::AlreadyFinished);
        };

        let status: String = task_row
            .try_get("status")
            .map_err(|error| format!("读取任务状态失败：{error}"))?;
        let Some(domain_status) = TaskStatus::parse(&status) else {
            tx.commit()
                .await
                .map_err(|error| format!("提交未知状态事务失败：{error}"))?;
            return Ok(ClaimExecutionOutcome::AlreadyFinished);
        };

        let mut domain_task = BacktestTask {
            id: task_id,
            user_id: None,
            status: domain_status,
        };

        if matches!(domain_status, TaskStatus::Succeeded | TaskStatus::Failed) {
            tx.commit()
                .await
                .map_err(|error| format!("提交重复任务事务失败：{error}"))?;
            return Ok(ClaimExecutionOutcome::AlreadyFinished);
        }

        if domain_status == TaskStatus::Running {
            let active = sqlx::query(
                r#"
                SELECT id, lease_expires_at <= clock_timestamp() AS expired
                FROM backtest_task_attempts
                WHERE task_id = $1 AND status = 'running'
                ORDER BY attempt_number DESC
                LIMIT 1
                FOR UPDATE
                "#,
            )
            .bind(task_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| format!("检查旧执行租约失败：{error}"))?;

            if let Some(active) = active {
                let expired: bool = active
                    .try_get("expired")
                    .map_err(|error| format!("读取租约状态失败：{error}"))?;
                if !expired {
                    tx.commit()
                        .await
                        .map_err(|error| format!("提交未过期租约事务失败：{error}"))?;
                    return Ok(ClaimExecutionOutcome::LeaseHeld);
                }

                let old_attempt_id: Uuid = active
                    .try_get("id")
                    .map_err(|error| format!("读取旧 Attempt ID 失败：{error}"))?;
                sqlx::query(
                    r#"
                    UPDATE backtest_task_attempts
                    SET status = $2, finished_at = NOW(),
                        error = 'worker lease expired'
                    WHERE id = $1 AND status = 'running'
                    "#,
                )
                .bind(old_attempt_id)
                .bind(AttemptStatus::Interrupted.as_str())
                .execute(&mut *tx)
                .await
                .map_err(|error| format!("标记过期 Attempt 失败：{error}"))?;
            }

            domain_task
                .transition(TaskStatus::Queued)
                .map_err(|error| format!("任务状态转换失败：{error}"))?;
        }

        domain_task
            .transition(TaskStatus::Running)
            .map_err(|error| format!("任务状态转换失败：{error}"))?;

        let config_json: String = sqlx::query_scalar(
            "SELECT config::text FROM backtest_jobs WHERE id = $1 AND task_id = $2",
        )
        .bind(job_id)
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| format!("读取回测 Job 配置失败：{error}"))?
        .ok_or_else(|| "消息中的 job_id 与任务不匹配".to_string())?;

        let config = serde_json::from_str(&config_json)
            .map_err(|error| format!("解析回测 Job 配置失败：{error}"))?;

        let attempt_number: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM backtest_task_attempts WHERE task_id = $1",
        )
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| format!("计算 Attempt 序号失败：{error}"))?;

        let attempt_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO backtest_task_attempts
                (id, task_id, attempt_number, status, worker_id, lease_expires_at)
            VALUES ($1, $2, $3, $4, $5, clock_timestamp() + ($6 * INTERVAL '1 second'))
            "#,
        )
        .bind(attempt_id)
        .bind(task_id)
        .bind(attempt_number)
        .bind(AttemptStatus::Running.as_str())
        .bind(worker_id)
        .bind(lease_seconds.clamp(10, 3600))
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("创建执行 Attempt 失败：{error}"))?;

        sqlx::query("UPDATE backtest_tasks SET status = $2, updated_at = NOW() WHERE id = $1")
            .bind(task_id)
            .bind(domain_task.status.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|error| format!("更新任务为 running 失败：{error}"))?;

        tx.commit()
            .await
            .map_err(|error| format!("提交任务认领事务失败：{error}"))?;

        Ok(ClaimExecutionOutcome::Claimed(ClaimedBacktestJob {
            task_id,
            attempt: BacktestAttempt {
                id: attempt_id,
                task_id,
                attempt_number,
                status: AttemptStatus::Running,
                error: None,
            },
            config,
        }))
    }

    /// 延长当前 Worker 持有的执行租约。
    pub async fn heartbeat_attempt(
        &self,
        attempt_id: Uuid,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<bool, String> {
        let result = sqlx::query(
            r#"
            UPDATE backtest_task_attempts
            SET heartbeat_at = clock_timestamp(),
                lease_expires_at = clock_timestamp() + ($3 * INTERVAL '1 second')
            WHERE id = $1
              AND worker_id = $2
              AND status = 'running'
              AND lease_expires_at > clock_timestamp()
            "#,
        )
        .bind(attempt_id)
        .bind(worker_id)
        .bind(lease_seconds.clamp(10, 3600))
        .execute(&self.pool)
        .await
        .map_err(|error| format!("更新执行心跳失败：{error}"))?;
        Ok(result.rows_affected() == 1)
    }

    /// 一个事务内保存结果、结束 Attempt 并结束 Task。
    pub async fn complete_success(
        &self,
        attempt_id: Uuid,
        task_id: Uuid,
        worker_id: &str,
        payload: &Value,
        duration_ms: i64,
    ) -> Result<(), String> {
        let record = BacktestResultRecord {
            task_id,
            schema_version: 1,
            payload: payload.clone(),
        };
        let payload_json = serde_json::to_string(&record.payload)
            .map_err(|error| format!("序列化回测结果失败：{error}"))?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| format!("开启成功结果事务失败：{error}"))?;

        let current_status: String =
            sqlx::query_scalar("SELECT status FROM backtest_tasks WHERE id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| format!("读取任务当前状态失败：{error}"))?
                .ok_or_else(|| "回测任务不存在".to_string())?;
        let mut task = BacktestTask {
            id: task_id,
            user_id: None,
            status: TaskStatus::parse(&current_status)
                .ok_or_else(|| format!("未知任务状态：{current_status}"))?,
        };
        task.transition(TaskStatus::Succeeded)
            .map_err(|error| format!("任务状态转换失败：{error}"))?;

        let attempt = sqlx::query(
            r#"
            UPDATE backtest_task_attempts
            SET status = $4, finished_at = NOW(), duration_ms = $5
            WHERE id = $1 AND task_id = $2 AND worker_id = $3
              AND status = 'running' AND lease_expires_at > clock_timestamp()
            "#,
        )
        .bind(attempt_id)
        .bind(task_id)
        .bind(worker_id)
        .bind(AttemptStatus::Succeeded.as_str())
        .bind(duration_ms.max(0))
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("更新成功 Attempt 失败：{error}"))?;
        if attempt.rows_affected() != 1 {
            return Err("当前 Worker 已不再拥有该执行 Attempt".to_string());
        }

        sqlx::query(
            r#"
            INSERT INTO backtest_results (task_id, schema_version, payload)
            VALUES ($1, $2, $3::jsonb)
            ON CONFLICT (task_id) DO UPDATE
            SET schema_version = EXCLUDED.schema_version,
                payload = EXCLUDED.payload,
                created_at = NOW()
            "#,
        )
        .bind(record.task_id)
        .bind(record.schema_version)
        .bind(payload_json)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("保存回测结果失败：{error}"))?;

        let task_update = sqlx::query(
            "UPDATE backtest_tasks SET status = $2, updated_at = NOW() WHERE id = $1 AND status = 'running'",
        )
        .bind(task_id)
        .bind(task.status.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("更新任务成功状态失败：{error}"))?;
        if task_update.rows_affected() != 1 {
            return Err("任务已不处于 running 状态，无法提交成功结果".to_string());
        }

        tx.commit()
            .await
            .map_err(|error| format!("提交成功结果事务失败：{error}"))
    }

    /// 一个事务内结束 Attempt 并记录 Task 失败状态。
    pub async fn complete_failure(
        &self,
        attempt_id: Uuid,
        task_id: Uuid,
        worker_id: &str,
        message: &str,
        duration_ms: Option<i64>,
    ) -> Result<(), String> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| format!("开启失败结果事务失败：{error}"))?;

        let current_status: String =
            sqlx::query_scalar("SELECT status FROM backtest_tasks WHERE id = $1 FOR UPDATE")
                .bind(task_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| format!("读取任务当前状态失败：{error}"))?
                .ok_or_else(|| "回测任务不存在".to_string())?;
        let mut task = BacktestTask {
            id: task_id,
            user_id: None,
            status: TaskStatus::parse(&current_status)
                .ok_or_else(|| format!("未知任务状态：{current_status}"))?,
        };
        task.transition(TaskStatus::Failed)
            .map_err(|error| format!("任务状态转换失败：{error}"))?;

        let attempt = sqlx::query(
            r#"
            UPDATE backtest_task_attempts
            SET status = $4, finished_at = NOW(), duration_ms = $5, error = $6
            WHERE id = $1 AND task_id = $2 AND worker_id = $3
              AND status = 'running' AND lease_expires_at > clock_timestamp()
            "#,
        )
        .bind(attempt_id)
        .bind(task_id)
        .bind(worker_id)
        .bind(AttemptStatus::Failed.as_str())
        .bind(duration_ms)
        .bind(message)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("更新失败 Attempt 失败：{error}"))?;
        if attempt.rows_affected() != 1 {
            return Err("当前 Worker 已不再拥有该执行 Attempt".to_string());
        }

        let task_update = sqlx::query(
            "UPDATE backtest_tasks SET status = $2, updated_at = NOW() WHERE id = $1 AND status = 'running'",
        )
        .bind(task_id)
        .bind(task.status.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("更新任务失败状态失败：{error}"))?;
        if task_update.rows_affected() != 1 {
            return Err("任务已不处于 running 状态，无法提交失败结果".to_string());
        }

        tx.commit()
            .await
            .map_err(|error| format!("提交失败结果事务失败：{error}"))
    }
}

#[async_trait]
impl BacktestExecutionRepository for PgBacktestRepository {
    async fn claim_execution(
        &self,
        task_id: Uuid,
        job_id: Uuid,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<ClaimExecutionOutcome, String> {
        PgBacktestRepository::claim_execution(self, task_id, job_id, worker_id, lease_seconds).await
    }

    async fn heartbeat_attempt(
        &self,
        attempt_id: Uuid,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<bool, String> {
        PgBacktestRepository::heartbeat_attempt(self, attempt_id, worker_id, lease_seconds).await
    }

    async fn complete_success(
        &self,
        attempt_id: Uuid,
        task_id: Uuid,
        worker_id: &str,
        payload: &Value,
        duration_ms: i64,
    ) -> Result<(), String> {
        PgBacktestRepository::complete_success(
            self,
            attempt_id,
            task_id,
            worker_id,
            payload,
            duration_ms,
        )
        .await
    }

    async fn complete_failure(
        &self,
        attempt_id: Uuid,
        task_id: Uuid,
        worker_id: &str,
        message: &str,
        duration_ms: Option<i64>,
    ) -> Result<(), String> {
        PgBacktestRepository::complete_failure(
            self,
            attempt_id,
            task_id,
            worker_id,
            message,
            duration_ms,
        )
        .await
    }
}
