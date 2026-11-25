use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use backtest_core::ExecutionMode;
use serde::Deserialize;

use crate::{
    app::state::AppState,
    http::error::{ApiError, api_error},
    infrastructure::database::backtest_repository as storage,
    modules::backtest::service::validate_dataset_name,
    tasks::{BacktestJob, BacktestTask, TaskStatus, make_task_id},
};

fn default_strategy() -> String {
    "sma_cross".to_string()
}

fn default_lot_size() -> u64 {
    100
}

fn default_stamp_duty_rate() -> f64 {
    0.0005
}

fn default_min_commission() -> f64 {
    5.0
}

#[derive(Debug, Deserialize)]
pub struct RunBacktestRequest {
    dataset: String,
    initial_cash: f64,
    commission_rate: f64,
    slippage_rate: f64,
    execution_mode: String,

    #[serde(default = "default_lot_size")]
    lot_size: u64,

    #[serde(default = "default_stamp_duty_rate")]
    stamp_duty_rate: f64,

    #[serde(default = "default_min_commission")]
    min_commission: f64,

    #[serde(default = "default_strategy")]
    strategy: String,

    #[serde(default)]
    short_window: Option<usize>,

    #[serde(default)]
    long_window: Option<usize>,
}

// GET /api/datasets
//
// data/cleaned/<symbol>/<date-range>/
//   ├── bars.csv
//   ├── rejected.csv
//   └── quality_report.json
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

    if request.lot_size == 0 {
        return Err(api_error(StatusCode::BAD_REQUEST, "每手股数必须大于 0"));
    }

    if !request.stamp_duty_rate.is_finite() || !(0.0..1.0).contains(&request.stamp_duty_rate) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "印花税率必须大于等于 0 且小于 1",
        ));
    }

    if !request.min_commission.is_finite() || request.min_commission < 0.0 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "最低佣金必须是有限的非负数",
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
        lot_size: request.lot_size,
        stamp_duty_rate: request.stamp_duty_rate,
        min_commission: request.min_commission,
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

// GET /api/backtests：最近 50 条回测任务。
pub async fn list_backtests(
    State(state): State<AppState>,
) -> Result<Json<Vec<BacktestTask>>, (StatusCode, Json<ApiError>)> {
    let ids = storage::list_task_ids(&state.db, 50)
        .await
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    let mut tasks = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(task) = storage::get_task(&state.db, &id)
            .await
            .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?
        {
            tasks.push(task);
        }
    }
    Ok(Json(tasks))
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
