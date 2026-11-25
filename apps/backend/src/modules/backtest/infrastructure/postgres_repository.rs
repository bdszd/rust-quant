use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::modules::backtest::{
    application::{ports::BacktestRepository, queries::BacktestTaskView},
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
