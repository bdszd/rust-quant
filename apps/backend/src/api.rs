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
use backtest_core::{ExecutionMode, load_bars};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct DatasetInfo {
    pub name: String,
    pub rows: usize,
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

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

// GET /api/datasets
pub async fn list_datasets(
    State(state): State<AppState>,
) -> Result<Json<Vec<DatasetInfo>>, (StatusCode, Json<ApiError>)> {
    let data_dir = state.data_dir.clone();

    let result = tokio::task::spawn_blocking(move || {
        let entries = std::fs::read_dir(&data_dir).map_err(|e| format!("无法读取数据目录：{e}"))?;

        let mut datasets = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("csv") {
                continue;
            }

            let Some(name) = path.file_name().and_then(|s| s.to_str()).map(str::to_owned) else {
                continue;
            };

            match load_bars(path.to_string_lossy().as_ref()) {
                Ok(bars) => datasets.push(DatasetInfo {
                    name,
                    rows: bars.len(),
                }),
                Err(_) => datasets.push(DatasetInfo { name, rows: 0 }),
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
    // 1. 校验数据集名称。
    validate_dataset_name(&request.dataset)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;

    // 2. 校验初始资金。
    if !request.initial_cash.is_finite() || request.initial_cash <= 0.0 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "初始资金必须是有限的正数",
        ));
    }

    // 3. 校验手续费率。
    if !request.commission_rate.is_finite() || !(0.0..1.0).contains(&request.commission_rate) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "手续费率必须大于等于 0 且小于 1",
        ));
    }

    // 4. 校验滑点率。
    if !request.slippage_rate.is_finite() || !(0.0..1.0).contains(&request.slippage_rate) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "滑点率必须大于等于 0 且小于 1",
        ));
    }

    // 5. 解析执行模式。
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

    // 6. 校验策略参数。
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

    // 7. 检查数据集文件是否存在。
    let path = state.data_dir.join(&request.dataset);

    if !path.is_file() {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("找不到数据集：{}", request.dataset),
        ));
    }

    // 9. 创建任务 ID。
    let id = make_task_id();

    // 10. 创建 API 任务记录。
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

    // 11. 创建完整的后台执行任务。
    let job = BacktestJob {
        id: id.clone(),
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

    // 12. 先持久化。
    //
    // 如果数据库写入失败，permit 会在函数返回时自动释放。
    storage::insert_task(&state.db, &task, &job)
        .await
        .map_err(|message| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("无法保存回测任务：{message}"),
            )
        })?;

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
