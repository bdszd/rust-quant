use crate::tasks::{ApiBacktestResult, BacktestJob, BacktestTask, TaskStatus};

use sqlx::{PgPool, Row, postgres::PgPoolOptions};

/// 建立 PostgreSQL 连接池。
///
/// DATABASE_URL 示例：
/// postgres://postgres:postgres@localhost:5432/rust_quant
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

/// 创建任务时，同时保存任务展示信息和完整执行参数。
pub async fn insert_task(
    pool: &PgPool,
    task: &BacktestTask,
    job: &BacktestJob,
) -> Result<(), String> {
    let job_json = serde_json::to_string(job).map_err(|e| format!("序列化回测任务失败：{e}"))?;

    sqlx::query(
        r#"
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
        "#,
    )
    .bind(&task.id)
    .bind(&task.dataset)
    .bind(&task.strategy)
    .bind(task.short_window.map(|v| v as i32))
    .bind(task.long_window.map(|v| v as i32))
    .bind(job_json)
    .bind(status_to_str(task.status))
    .execute(pool)
    .await
    .map_err(|e| format!("保存回测任务失败：{e}"))?;

    Ok(())
}

/// 原子认领任务。
///
/// 只有状态仍然是 queued 的任务才能被认领。
/// 认领成功时记录 started_at。
pub async fn claim_task(pool: &PgPool, id: &str) -> Result<bool, sqlx::Error> {
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
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// 服务启动时，将上次进程遗留的 running 任务重新设为 queued。
///
/// 当前阶段仍按单个 Backend 实例运行；引入多实例 Worker 后，
/// 将由任务租约/超时恢复机制替代全量重置。
pub async fn reset_running_tasks(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'queued',
            started_at = NULL,
            finished_at = NULL,
            execution_duration_ms = NULL,
            error = NULL,
            updated_at = NOW()
        WHERE status = 'running'
          AND job_json IS NOT NULL
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// 从数据库读取所有可以恢复执行的任务。
pub async fn list_queued_jobs(pool: &PgPool) -> Result<Vec<BacktestJob>, String> {
    let rows = sqlx::query(
        r#"
        SELECT job_json
        FROM backtest_tasks
        WHERE status = 'queued'
          AND job_json IS NOT NULL
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询待执行任务失败：{e}"))?;

    let mut jobs = Vec::with_capacity(rows.len());

    for row in rows {
        let job_json: String = row
            .try_get("job_json")
            .map_err(|e| format!("读取任务参数失败：{e}"))?;

        let job: BacktestJob =
            serde_json::from_str(&job_json).map_err(|e| format!("解析待恢复任务失败：{e}"))?;

        jobs.push(job);
    }

    Ok(jobs)
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
            .map_err(|e| format!("读取任务错误信息失败：{e}"))?,
        execution_duration_ms,
    }))
}

/// 保存成功结果和实际执行耗时。
pub async fn mark_succeeded(
    pool: &PgPool,
    id: &str,
    result: &ApiBacktestResult,
    execution_duration_ms: i64,
) -> Result<(), String> {
    let result_json =
        serde_json::to_string(result).map_err(|e| format!("序列化回测结果失败：{e}"))?;

    sqlx::query(
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
    .execute(pool)
    .await
    .map_err(|e| format!("保存回测结果失败：{e}"))?;

    Ok(())
}

/// 保存失败状态和实际执行耗时。
///
/// 如果线程异常，可能没有拿到有效耗时，此时传入 None。
pub async fn mark_failed(
    pool: &PgPool,
    id: &str,
    message: &str,
    execution_duration_ms: Option<i64>,
) -> Result<(), sqlx::Error> {
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
    .execute(pool)
    .await?;

    Ok(())
}
