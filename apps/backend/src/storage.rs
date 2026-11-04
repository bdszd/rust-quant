
use crate::tasks::{
    ApiBacktestResult,
    BacktestJob,
    BacktestTask,
    TaskStatus,
};

use sqlx::{
    Row,
    SqlitePool,
    sqlite::{
        SqliteConnectOptions,
        SqliteJournalMode,
        SqlitePoolOptions,
    },
};

use std::{path::Path, time::Duration};

pub async fn connect(database_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
}

pub async fn init(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS backtest_tasks (
            id TEXT PRIMARY KEY,
            dataset TEXT NOT NULL,
            strategy TEXT NOT NULL,
            short_window INTEGER,
            long_window INTEGER,
            job_json TEXT,
            status TEXT NOT NULL,
            result_json TEXT,
            error TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            started_at TEXT,
            finished_at TEXT,
            execution_duration_ms INTEGER
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 兼容旧数据库：检查 job_json 列是否已经存在。
    let job_exists: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM pragma_table_info('backtest_tasks')
        WHERE name = 'job_json'
        "#,
    )
    .fetch_one(pool)
    .await?;

    if job_exists == 0 {
        sqlx::query(
            "ALTER TABLE backtest_tasks ADD COLUMN job_json TEXT",
        )
        .execute(pool)
        .await?;
    }

    // 兼容旧数据库：检查 started_at 列。
    let start_exists: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM pragma_table_info('backtest_tasks')
        WHERE name = 'started_at'
        "#,
    )
    .fetch_one(pool)
    .await?;

    if start_exists == 0 {
        sqlx::query(
            "ALTER TABLE backtest_tasks ADD COLUMN started_at TEXT",
        )
        .execute(pool)
        .await?;
    }

    // 兼容旧数据库：检查 finished_at 列。
    let finish_exists: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM pragma_table_info('backtest_tasks')
        WHERE name = 'finished_at'
        "#,
    )
    .fetch_one(pool)
    .await?;

    if finish_exists == 0 {
        sqlx::query(
            "ALTER TABLE backtest_tasks ADD COLUMN finished_at TEXT",
        )
        .execute(pool)
        .await?;
    }

    // 兼容旧数据库：检查 execution_duration_ms 列。
    let duration_exists: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM pragma_table_info('backtest_tasks')
        WHERE name = 'execution_duration_ms'
        "#,
    )
    .fetch_one(pool)
    .await?;

    if duration_exists == 0 {
        sqlx::query(
            "ALTER TABLE backtest_tasks ADD COLUMN execution_duration_ms INTEGER",
        )
        .execute(pool)
        .await?;
    }

    Ok(())
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
    pool: &SqlitePool,
    task: &BacktestTask,
    job: &BacktestJob,
) -> Result<(), String> {
    let job_json = serde_json::to_string(job)
        .map_err(|e| format!("序列化回测任务失败：{e}"))?;

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
        VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL)
        "#,
    )
    .bind(&task.id)
    .bind(&task.dataset)
    .bind(&task.strategy)
    .bind(task.short_window.map(|v| v as i64))
    .bind(task.long_window.map(|v| v as i64))
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
pub async fn claim_task(
    pool: &SqlitePool,
    id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'running',
            started_at = CURRENT_TIMESTAMP,
            finished_at = NULL,
            execution_duration_ms = NULL,
            result_json = NULL,
            error = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
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
/// 注意：当前方案假设同一时间只有一个服务实例负责这张任务表。
pub async fn reset_running_tasks(
    pool: &SqlitePool,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'queued',
            started_at = NULL,
            finished_at = NULL,
            execution_duration_ms = NULL,
            error = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE status = 'running'
          AND job_json IS NOT NULL
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// 从数据库读取所有可以恢复执行的任务。
pub async fn list_queued_jobs(
    pool: &SqlitePool,
) -> Result<Vec<BacktestJob>, String> {
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

        let job: BacktestJob = serde_json::from_str(&job_json)
            .map_err(|e| format!("解析待恢复任务失败：{e}"))?;

        jobs.push(job);
    }

    Ok(jobs)
}

pub async fn get_task(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<BacktestTask>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, dataset, strategy, short_window, long_window,
               status, result_json, error, execution_duration_ms
        FROM backtest_tasks
        WHERE id = ?
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

    let short_window: Option<i64> = row
        .try_get("short_window")
        .map_err(|e| format!("读取短期窗口失败：{e}"))?;

    let long_window: Option<i64> = row
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
    pool: &SqlitePool,
    id: &str,
    result: &ApiBacktestResult,
    execution_duration_ms: i64,
) -> Result<(), String> {
    let result_json = serde_json::to_string(result)
        .map_err(|e| format!("序列化回测结果失败：{e}"))?;

    sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'succeeded',
            result_json = ?,
            error = NULL,
            finished_at = CURRENT_TIMESTAMP,
            execution_duration_ms = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
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
    pool: &SqlitePool,
    id: &str,
    message: &str,
    execution_duration_ms: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE backtest_tasks
        SET status = 'failed',
            result_json = NULL,
            error = ?,
            finished_at = CURRENT_TIMESTAMP,
            execution_duration_ms = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
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
