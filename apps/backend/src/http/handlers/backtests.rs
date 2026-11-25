use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    app::state::AppState,
    http::{
        dto::backtest::BacktestTaskResponse,
        error::{ApiError, api_error},
        middleware::authentication::CurrentUser,
    },
    modules::backtest::{
        application::{commands::CreateBacktestCommand, error::BacktestApplicationError},
        domain::{BacktestJobConfig, ExecutionMode, StrategyConfig},
    },
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

/// POST /api/backtests
///
/// Handler 只负责 HTTP 参数解析、转换应用层命令和组织 HTTP 响应。
pub async fn create_backtest(
    State(state): State<AppState>,
    Extension(current_user): Extension<CurrentUser>,
    Json(request): Json<RunBacktestRequest>,
) -> Result<(StatusCode, Json<BacktestTaskResponse>), (StatusCode, Json<ApiError>)> {
    let request_started = std::time::Instant::now();

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
            (
                StrategyConfig::SmaCross {
                    short_window: short,
                    long_window: long,
                },
                Some(short),
                Some(long),
            )
        }
        "buy_and_hold" => (StrategyConfig::BuyAndHold, None, None),
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "不支持的策略，可选值：sma_cross、buy_and_hold",
            ));
        }
    };

    let strategy_name = match strategy {
        StrategyConfig::SmaCross { .. } => "sma_cross",
        StrategyConfig::BuyAndHold => "buy_and_hold",
    }
    .to_string();

    let config = BacktestJobConfig {
        dataset: request.dataset.clone(),
        initial_cash: request.initial_cash,
        commission_rate: request.commission_rate,
        slippage_rate: request.slippage_rate,
        lot_size: request.lot_size,
        stamp_duty_rate: request.stamp_duty_rate,
        min_commission: request.min_commission,
        execution_mode,
        strategy,
    };

    let task = state
        .backtest
        .create_task(CreateBacktestCommand {
            user_id: Some(current_user.user_id),
            config,
        })
        .await
        .map_err(|error| match error {
            BacktestApplicationError::InvalidConfig(message) => {
                api_error(StatusCode::BAD_REQUEST, message)
            }
            BacktestApplicationError::DatasetNotFound(dataset) => api_error(
                StatusCode::NOT_FOUND,
                format!("找不到清洗后的数据集：{dataset}"),
            ),
            BacktestApplicationError::DatasetCatalog(message)
            | BacktestApplicationError::Repository(message) => {
                api_error(StatusCode::INTERNAL_SERVER_ERROR, message.to_string())
            }
        })?;

    state.metrics.increment_tasks_accepted();
    state
        .metrics
        .record_duration("api_create_backtest", request_started.elapsed());

    Ok((
        StatusCode::ACCEPTED,
        Json(BacktestTaskResponse::queued(
            &task,
            request.dataset,
            strategy_name,
            short_window,
            long_window,
        )),
    ))
}

/// GET /api/backtests：最近 50 条回测任务。
pub async fn list_backtests(
    State(state): State<AppState>,
) -> Result<Json<Vec<BacktestTaskResponse>>, (StatusCode, Json<ApiError>)> {
    let tasks = state
        .backtest
        .list_tasks(50)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    Ok(Json(
        tasks
            .into_iter()
            .map(BacktestTaskResponse::from_view)
            .collect(),
    ))
}

/// GET /api/backtests/{id}
pub async fn get_backtest(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<BacktestTaskResponse>, (StatusCode, Json<ApiError>)> {
    let task_id = BacktestTaskResponse::parse_id(&id)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "回测任务 ID 格式无效"))?;

    let task = state
        .backtest
        .get_task(task_id)
        .await
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    task.map(|view| Json(BacktestTaskResponse::from_view(view)))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("找不到回测任务：{id}")))
}
