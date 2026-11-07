use crate::{
    metrics::Metrics,
    tasks::{ApiBacktestResult, BacktestJob, BacktestTask, TaskStatus},
};

use sqlx::{Acquire, PgPool, Row, postgres::PgPoolOptions};
use std::time::Instant;

#[derive(Debug)]
pub struct OutboxMessage {
    pub task_id: String,
    pub payload: String,
}

/// 建立 PostgreSQL 连接池。
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(database_url)
        .await
}

fn status_to_str(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Queued => "queued",
        TaskStatus::Running => "running",
        TaskStatus::Succeeded => "succeeded",
        TaskStatus::Failed => "failed",
    }
}

fn status_from_str(status: &str) -> Result<TaskStatus, String> {
    match status {
        "queued" => Ok(TaskStatus::Queued),
        "running" => Ok(TaskStatus::Running),
        "succeeded" => Ok(TaskStatus::Succeeded),
        "failed" => Ok(TaskStatus::Failed),
        other => Err(format!("数据库中存在未知任务状态：{other}")),
    }
}

/// 在一条 PostgreSQL SQL 语句中原子写入任务和 Outbox 事件。
///
/// 细分统计：
/// - db_serialize_job：任务 JSON 序列化
/// - db_acquire_insert_task：获取连接池连接
/// - db_task_outbox_insert：同时插入任务和 Outbox
///
/// 单条 SQL 语句本身具有原子性，因此不再需要显式开启事务。
pub async fn insert_task(
    pool: &PgPool,
    task: &BacktestTask,
    job: &BacktestJob,
    metrics: &Metrics,
) -> Result<(), String> {
    // 在获取数据库连接之前完成 JSON 序列化。
    let started = Instant::now();
    let job_json = serde_json::to_string(job).map_err(|e| format!("序列化回测任务失败：{e}"))?;
    metrics.record_duration("db_serialize_job", started.elapsed());

    // 获取连接。
    let started = Instant::now();
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| format!("获取数据库连接失败：{e}"))?;
    metrics.record_duration("db_acquire_insert_task", started.elapsed());

    // 一条 SQL 完成两次插入。
    //
    // 如果第二次 INSERT 失败，整条 SQL 都会回滚，
    // 不会留下只有任务记录、没有 Outbox 事件的数据。
    let started = Instant::now();

    sqlx::query(
        r#"
        WITH inserted_task AS (
            INSERT INTO backtest_tasks (
                id,
                dataset,
                strategy,
                short_window,
                long_window,
                job_json,
                status,
                result_json,
                error
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, NULL)
            RETURNING id
        )
        INSERT INTO backtest_outbox (task_id, payload)
        SELECT id, $8
        FROM inserted_task
        "#,
    )
    .bind(&task.id)
    .bind(&task.dataset)
    .bind(&task.strategy)
    .bind(task.short_window.map(|v| v as i32))
    .bind(task.long_window.map(|v| v as i32))
    .bind(&job_json)
    .bind(status_to_str(task.status))
    .bind(&job_json)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("保存回测任务及 Outbox 事件失败：{e}"))?;

    metrics.record_duration("db_task_outbox_insert", started.elapsed());

    Ok(())
}

/// 启动恢复：
/// 1. 把上次进程遗留的 running 任务重置为 queued，并重置其 Outbox；
/// 2. 为升级前已经存在、但尚无 Outbox 记录的 queued 任务补建事件。
///
/// 这是单 Backend 实例的恢复策略。多实例部署必须改为租约/心跳恢复。
pub async fn reset_running_tasks(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let rows = sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'queued',
            started_at = NULL,
            finished_at = NULL,
            execution_duration_ms = NULL,
            result_json = NULL,
            error = NULL,
            updated_at = NOW()
        WHERE status = 'running'
          AND job_json IS NOT NULL
        RETURNING id, job_json
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;

    for row in &rows {
        let id: String = row.try_get("id")?;
        let job_json: String = row.try_get("job_json")?;

        sqlx::query(
            r#"
            INSERT INTO backtest_outbox (task_id, payload, created_at, published_at)
            VALUES ($1, $2, NOW(), NULL)
            ON CONFLICT (task_id)
            DO UPDATE SET
                payload = EXCLUDED.payload,
                created_at = NOW(),
                published_at = NULL
            "#,
        )
        .bind(id)
        .bind(job_json)
        .execute(&mut *tx)
        .await?;
    }

    // 兼容引入 Outbox 前已经写入数据库的 queued 任务。
    sqlx::query(
        r#"
        INSERT INTO backtest_outbox (task_id, payload)
        SELECT id, job_json
        FROM backtest_tasks
        WHERE status = 'queued'
          AND job_json IS NOT NULL
        ON CONFLICT (task_id) DO NOTHING
        "#,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(rows.len() as u64)
}

/// 读取尚未收到 RabbitMQ Publisher Confirm 的 Outbox 事件。
pub async fn list_pending_outbox(pool: &PgPool, limit: i64) -> Result<Vec<OutboxMessage>, String> {
    let rows = sqlx::query(
        r#"
        SELECT task_id, payload
        FROM backtest_outbox
        WHERE published_at IS NULL
        ORDER BY created_at ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询待发布 Outbox 失败：{e}"))?;

    let mut messages = Vec::with_capacity(rows.len());

    for row in rows {
        messages.push(OutboxMessage {
            task_id: row
                .try_get("task_id")
                .map_err(|e| format!("读取 Outbox task_id 失败：{e}"))?,
            payload: row
                .try_get("payload")
                .map_err(|e| format!("读取 Outbox payload 失败：{e}"))?,
        });
    }

    Ok(messages)
}

/// 批量标记已经收到 RabbitMQ Publisher Confirm 的 Outbox 事件。
///
/// 只更新尚未标记为 published 的记录。
pub async fn mark_outbox_published_batch(
    pool: &PgPool,
    task_ids: &[String],
) -> Result<u64, sqlx::Error> {
    if task_ids.is_empty() {
        return Ok(0);
    }

    let result = sqlx::query(
        r#"
        UPDATE backtest_outbox
        SET published_at = NOW()
        WHERE task_id = ANY($1)
          AND published_at IS NULL
        "#,
    )
    .bind(task_ids.to_vec())
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// 单条标记接口，保留以兼容其他调用位置。
pub async fn mark_outbox_published(pool: &PgPool, task_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE backtest_outbox
        SET published_at = NOW()
        WHERE task_id = $1
          AND published_at IS NULL
        "#,
    )
    .bind(task_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// 原子认领任务。只有 queued 状态的任务才能转为 running。
pub async fn claim_task(pool: &PgPool, id: &str, metrics: &Metrics) -> Result<bool, sqlx::Error> {
    let started = Instant::now();
    let mut conn = pool.acquire().await?;
    metrics.record_duration("db_acquire_claim_task", started.elapsed());

    let started = Instant::now();
    let result = sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'running',
            started_at = NOW(),
            finished_at = NULL,
            execution_duration_ms = NULL,
            result_json = NULL,
            error = NULL,
            updated_at = NOW()
        WHERE id = $1
          AND status = 'queued'
          AND job_json IS NOT NULL
        "#,
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    metrics.record_duration("db_claim_update", started.elapsed());

    Ok(result.rows_affected() == 1)
}

/// 最近回测任务 ID，供工作台展示历史记录。
pub async fn list_task_ids(pool: &PgPool, limit: i64) -> Result<Vec<String>, String> {
    sqlx::query_scalar("SELECT id FROM backtest_tasks ORDER BY created_at DESC LIMIT $1")
        .bind(limit.clamp(1, 100))
        .fetch_all(pool)
        .await
        .map_err(|e| format!("查询回测历史失败：{e}"))
}

pub async fn get_task(pool: &PgPool, id: &str) -> Result<Option<BacktestTask>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, dataset, strategy, short_window, long_window,
               status, result_json, error, execution_duration_ms
        FROM backtest_tasks
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询回测任务失败：{e}"))?;

    let Some(row) = row else {
        return Ok(None);
    };

    let status: String = row
        .try_get("status")
        .map_err(|e| format!("读取任务状态失败：{e}"))?;

    let result_json: Option<String> = row
        .try_get("result_json")
        .map_err(|e| format!("读取回测结果失败：{e}"))?;

    let result: Option<ApiBacktestResult> = result_json
        .map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|e| format!("解析数据库中的回测结果失败：{e}"))?;

    let short_window: Option<i32> = row
        .try_get("short_window")
        .map_err(|e| format!("读取短期窗口失败：{e}"))?;

    let long_window: Option<i32> = row
        .try_get("long_window")
        .map_err(|e| format!("读取长期窗口失败：{e}"))?;

    let execution_duration_ms: Option<i64> = row
        .try_get("execution_duration_ms")
        .map_err(|e| format!("读取执行耗时失败：{e}"))?;

    Ok(Some(BacktestTask {
        id: row
            .try_get("id")
            .map_err(|e| format!("读取任务 ID 失败：{e}"))?,
        dataset: row
            .try_get("dataset")
            .map_err(|e| format!("读取数据集失败：{e}"))?,
        strategy: row
            .try_get("strategy")
            .map_err(|e| format!("读取策略失败：{e}"))?,
        short_window: short_window.map(|v| v as usize),
        long_window: long_window.map(|v| v as usize),
        status: status_from_str(&status)?,
        result,
        error: row
            .try_get("error")
            .map_err(|e| format!("读取错误信息失败：{e}"))?,
        execution_duration_ms,
    }))
}

/// 保存成功结果和实际执行耗时。
pub async fn mark_succeeded(
    pool: &PgPool,
    id: &str,
    result: &ApiBacktestResult,
    execution_duration_ms: i64,
    metrics: &Metrics,
) -> Result<(), String> {
    let started = Instant::now();
    let result_json =
        serde_json::to_string(result).map_err(|e| format!("序列化回测结果失败：{e}"))?;
    metrics.record_duration("db_serialize_result", started.elapsed());

    let started = Instant::now();
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| format!("获取结果持久化连接失败：{e}"))?;
    metrics.record_duration("db_acquire_result_persist", started.elapsed());

    let started = Instant::now();
    let update = sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'succeeded',
            result_json = $1,
            error = NULL,
            finished_at = NOW(),
            execution_duration_ms = $2,
            updated_at = NOW()
        WHERE id = $3
          AND status = 'running'
        "#,
    )
    .bind(result_json)
    .bind(execution_duration_ms)
    .bind(id)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("保存回测结果失败：{e}"))?;
    metrics.record_duration("db_result_update", started.elapsed());

    if update.rows_affected() != 1 {
        return Err(format!("任务 {id} 不处于 running 状态，未能保存成功结果"));
    }

    Ok(())
}

/// 保存失败状态和实际执行耗时。
pub async fn mark_failed(
    pool: &PgPool,
    id: &str,
    message: &str,
    execution_duration_ms: Option<i64>,
    metrics: &Metrics,
) -> Result<(), sqlx::Error> {
    let started = Instant::now();
    let mut conn = pool.acquire().await?;
    metrics.record_duration("db_acquire_result_persist", started.elapsed());

    let started = Instant::now();
    sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'failed',
            result_json = NULL,
            error = $1,
            finished_at = NOW(),
            execution_duration_ms = $2,
            updated_at = NOW()
        WHERE id = $3
          AND status = 'running'
        "#,
    )
    .bind(message)
    .bind(execution_duration_ms)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    metrics.record_duration("db_result_update", started.elapsed());

    Ok(())
}
