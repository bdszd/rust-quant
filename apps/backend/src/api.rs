use crate::{
    backtest::validate_dataset_name,
    state::AppState,
    storage,
    tasks::{BacktestJob, BacktestTask, TaskStatus, make_task_id},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use backtest_core::ExecutionMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct DatasetInfo {
    /// 股票代码/日期范围，例如 000001.SZ/20240101_20241231。
    pub name: String,
    /// 从 quality_report.json 的 accepted_rows 读取。
    pub rows: usize,
}

#[derive(Debug, Deserialize)]
struct DatasetMetadata {
    accepted_rows: usize,
}

fn default_strategy() -> String {
    "sma_cross".to_string()
}

#[derive(Debug, Deserialize)]
pub struct RunBacktestRequest {
    dataset: String,
    initial_cash: f64,
    commission_rate: f64,
    slippage_rate: f64,
    execution_mode: String,

    #[serde(default = "default_strategy")]
    strategy: String,

    #[serde(default)]
    short_window: Option<usize>,

    #[serde(default)]
    long_window: Option<usize>,
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: message.into(),
        }),
    )
}

pub async fn get_metrics(State(state): State<AppState>) -> Json<crate::metrics::MetricsSnapshot> {
    let pool_size = state.db.size() as usize;
    let pool_idle = state.db.num_idle();

    let pool_stats = crate::metrics::DatabasePoolStats {
        size: pool_size,
        idle: pool_idle,
        busy: pool_size.saturating_sub(pool_idle),
        max_connections: state.db.options().get_max_connections() as usize,
    };

    Json(state.metrics.snapshot(pool_stats))
}

/// GET /metrics：Prometheus text exposition format。
///
/// 保留 /api/metrics 供现有 JSON 调试使用；Prometheus 使用独立的标准抓取端点。
pub async fn prometheus_metrics(State(state): State<AppState>) -> (axum::http::HeaderMap, String) {
    use std::fmt::Write;

    let pool_size = state.db.size() as usize;
    let pool_idle = state.db.num_idle();
    let pool_stats = crate::metrics::DatabasePoolStats {
        size: pool_size,
        idle: pool_idle,
        busy: pool_size.saturating_sub(pool_idle),
        max_connections: state.db.options().get_max_connections() as usize,
    };
    let snapshot = state.metrics.snapshot(pool_stats);

    let mut body = String::new();
    macro_rules! metric {
        ($name:literal, $help:literal, $kind:literal, $value:expr) => {{
            let _ = writeln!(body, "# HELP {} {}", $name, $help);
            let _ = writeln!(body, "# TYPE {} {}", $name, $kind);
            let _ = writeln!(body, "{} {}", $name, $value);
        }};
    }

    metric!(
        "rust_quant_uptime_seconds",
        "Backend process uptime in seconds.",
        "gauge",
        snapshot.uptime_seconds
    );
    metric!(
        "rust_quant_worker_count",
        "Configured number of workers.",
        "gauge",
        snapshot.worker_count
    );
    metric!(
        "rust_quant_worker_active",
        "Currently active workers.",
        "gauge",
        snapshot.worker_active
    );
    metric!(
        "rust_quant_rabbitmq_connected",
        "Whether at least one RabbitMQ connection is open.",
        "gauge",
        u8::from(snapshot.rabbitmq_connected)
    );
    metric!(
        "rust_quant_db_pool_connections",
        "Current PostgreSQL pool connections.",
        "gauge",
        snapshot.database_pool.size
    );
    metric!(
        "rust_quant_db_pool_idle",
        "Idle PostgreSQL pool connections.",
        "gauge",
        snapshot.database_pool.idle
    );
    metric!(
        "rust_quant_db_pool_busy",
        "Busy PostgreSQL pool connections.",
        "gauge",
        snapshot.database_pool.busy
    );
    metric!(
        "rust_quant_db_pool_max_connections",
        "Maximum PostgreSQL pool connections.",
        "gauge",
        snapshot.database_pool.max_connections
    );

    metric!(
        "rust_quant_tasks_accepted_total",
        "Total accepted backtest tasks since process start.",
        "counter",
        snapshot.tasks.accepted
    );
    metric!(
        "rust_quant_tasks_succeeded_total",
        "Total succeeded backtest tasks since process start.",
        "counter",
        snapshot.tasks.succeeded
    );
    metric!(
        "rust_quant_tasks_failed_total",
        "Total failed backtest tasks since process start.",
        "counter",
        snapshot.tasks.failed
    );
    metric!(
        "rust_quant_tasks_duplicate_total",
        "Total duplicate task deliveries since process start.",
        "counter",
        snapshot.tasks.duplicate
    );

    metric!(
        "rust_quant_rabbitmq_publish_total",
        "Total RabbitMQ publish attempts.",
        "counter",
        snapshot.messaging.publish_total
    );
    metric!(
        "rust_quant_rabbitmq_publish_failed_total",
        "Total RabbitMQ publish failures.",
        "counter",
        snapshot.messaging.publish_failed
    );
    metric!(
        "rust_quant_rabbitmq_publish_confirm_failed_total",
        "Total RabbitMQ publisher confirm failures.",
        "counter",
        snapshot.messaging.publish_confirm_failed
    );
    metric!(
        "rust_quant_rabbitmq_consume_total",
        "Total RabbitMQ message deliveries.",
        "counter",
        snapshot.messaging.consume_total
    );
    metric!(
        "rust_quant_rabbitmq_ack_total",
        "Total successful RabbitMQ acknowledgements.",
        "counter",
        snapshot.messaging.ack_total
    );
    metric!(
        "rust_quant_rabbitmq_ack_failed_total",
        "Total RabbitMQ acknowledgement failures.",
        "counter",
        snapshot.messaging.ack_failed
    );
    metric!(
        "rust_quant_rabbitmq_reject_total",
        "Total rejected RabbitMQ messages.",
        "counter",
        snapshot.messaging.reject_total
    );

    let _ = writeln!(
        body,
        "# HELP rust_quant_stage_duration_window_ms Rolling-window stage duration statistics in milliseconds."
    );
    let _ = writeln!(body, "# TYPE rust_quant_stage_duration_window_ms gauge");
    for (stage, stats) in snapshot.stages {
        for (stat, value) in [
            ("avg", stats.avg_ms),
            ("min", stats.min_ms),
            ("max", stats.max_ms),
            ("p50", stats.p50_ms),
            ("p95", stats.p95_ms),
            ("p99", stats.p99_ms),
        ] {
            let _ = writeln!(
                body,
                "rust_quant_stage_duration_window_ms{{stage=\"{}\",stat=\"{}\"}} {}",
                stage, stat, value
            );
        }
    }

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    (headers, body)
}

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

// GET /api/datasets
//
// data/cleaned/<symbol>/<date-range>/
//   ├── bars.csv
//   ├── rejected.csv
//   └── quality_report.json
pub async fn list_datasets(
    State(state): State<AppState>,
) -> Result<Json<Vec<DatasetInfo>>, (StatusCode, Json<ApiError>)> {
    let data_dir = state.data_dir.clone();

    let result = tokio::task::spawn_blocking(move || {
        let symbols =
            std::fs::read_dir(&data_dir).map_err(|e| format!("无法读取清洗数据目录：{e}"))?;

        let mut datasets = Vec::new();

        for symbol_entry in symbols {
            let symbol_entry = symbol_entry.map_err(|e| format!("读取股票代码目录失败：{e}"))?;
            let symbol_path = symbol_entry.path();

            if !symbol_path.is_dir() {
                continue;
            }

            let Some(symbol) = symbol_path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };

            let date_ranges = std::fs::read_dir(&symbol_path)
                .map_err(|e| format!("读取 {symbol} 的日期目录失败：{e}"))?;

            for date_entry in date_ranges {
                let date_entry = date_entry.map_err(|e| format!("读取日期目录失败：{e}"))?;
                let date_path = date_entry.path();

                if !date_path.is_dir() {
                    continue;
                }

                let Some(date_range) = date_path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };

                let bars_path = date_path.join("bars.csv");
                let metadata_path = date_path.join("quality_report.json");

                if !bars_path.is_file() || !metadata_path.is_file() {
                    continue;
                }

                let metadata_content = std::fs::read_to_string(&metadata_path)
                    .map_err(|e| format!("读取数据集元数据失败：{metadata_path:?}: {e}"))?;

                let metadata: DatasetMetadata = serde_json::from_str(&metadata_content)
                    .map_err(|e| format!("解析数据集元数据失败：{metadata_path:?}: {e}"))?;

                datasets.push(DatasetInfo {
                    name: format!("{symbol}/{date_range}"),
                    rows: metadata.accepted_rows,
                });
            }
        }

        datasets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok::<_, String>(datasets)
    })
    .await
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("数据集扫描任务异常：{e}"),
        )
    })?
    .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    Ok(Json(result))
}

// POST /api/backtests
pub async fn create_backtest(
    State(state): State<AppState>,
    Json(request): Json<RunBacktestRequest>,
) -> Result<(StatusCode, Json<BacktestTask>), (StatusCode, Json<ApiError>)> {
    let request_started = std::time::Instant::now();

    validate_dataset_name(&request.dataset)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;

    if !request.initial_cash.is_finite() || request.initial_cash <= 0.0 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "初始资金必须是有限的正数",
        ));
    }

    if !request.commission_rate.is_finite() || !(0.0..1.0).contains(&request.commission_rate) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "手续费率必须大于等于 0 且小于 1",
        ));
    }

    if !request.slippage_rate.is_finite() || !(0.0..1.0).contains(&request.slippage_rate) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "滑点率必须大于等于 0 且小于 1",
        ));
    }

    let execution_mode = match request.execution_mode.as_str() {
        "next_open" => ExecutionMode::NextOpen,
        "next_close" => ExecutionMode::NextClose,
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "execution_mode 只能是 next_open 或 next_close",
            ));
        }
    };

    let (strategy, short_window, long_window) = match request.strategy.as_str() {
        "sma_cross" => {
            let short = request.short_window.unwrap_or(2);
            let long = request.long_window.unwrap_or(3);

            if short == 0 {
                return Err(api_error(StatusCode::BAD_REQUEST, "短期均线窗口必须大于 0"));
            }

            if long <= short {
                return Err(api_error(
                    StatusCode::BAD_REQUEST,
                    "长期均线窗口必须大于短期均线窗口",
                ));
            }

            ("sma_cross".to_string(), Some(short), Some(long))
        }
        "buy_and_hold" => ("buy_and_hold".to_string(), None, None),
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "不支持的策略，可选值：sma_cross、buy_and_hold",
            ));
        }
    };

    let dataset_dir = state.data_dir.join(&request.dataset);
    if !dataset_dir.join("bars.csv").is_file() || !dataset_dir.join("quality_report.json").is_file()
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("找不到清洗后的数据集：{}", request.dataset),
        ));
    }

    let id = make_task_id();

    let task = BacktestTask {
        id: id.clone(),
        dataset: request.dataset.clone(),
        strategy: strategy.clone(),
        short_window,
        long_window,
        status: TaskStatus::Queued,
        result: None,
        error: None,
        execution_duration_ms: None,
    };

    let job = BacktestJob {
        id,
        dataset: request.dataset,
        data_dir: state.data_dir.clone(),
        initial_cash: request.initial_cash,
        commission_rate: request.commission_rate,
        slippage_rate: request.slippage_rate,
        execution_mode,
        strategy,
        short_window,
        long_window,
    };

    // 任务记录和 Outbox 消息在同一个 PostgreSQL 事务中提交。
    let db_started = std::time::Instant::now();
    storage::insert_task(&state.db, &task, &job, &state.metrics)
        .await
        .map_err(|message| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("无法保存回测任务：{message}"),
            )
        })?;

    state
        .metrics
        .record_duration("db_insert_task", db_started.elapsed());
    state
        .metrics
        .record_duration("api_create_backtest", request_started.elapsed());
    state.metrics.increment_tasks_accepted();

    Ok((StatusCode::ACCEPTED, Json(task)))
}

// GET /api/backtests/{id}
pub async fn get_backtest(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<BacktestTask>, (StatusCode, Json<ApiError>)> {
    let task = storage::get_task(&state.db, &id)
        .await
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    task.map(Json)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("找不到回测任务：{id}")))
}
