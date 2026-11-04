use backtest_core::{BacktestResult, ExecutionMode, PortfolioSnapshot, Side, Trade};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BacktestTask {
    pub id: String,
    pub dataset: String,
    pub strategy: String,
    pub short_window: Option<usize>,
    pub long_window: Option<usize>,
    pub status: TaskStatus,
    pub result: Option<ApiBacktestResult>,
    pub error: Option<String>,

    // 回测实际执行耗时，单位为毫秒。
    // None 表示尚无可用的执行耗时。
    #[serde(default)]
    pub execution_duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJob {
    pub id: String,
    pub dataset: String,
    pub data_dir: PathBuf,
    pub initial_cash: f64,
    pub commission_rate: f64,
    pub slippage_rate: f64,
    pub execution_mode: ExecutionMode,
    pub strategy: String,
    pub short_window: Option<usize>,
    pub long_window: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiBacktestResult {
    pub id: String,
    pub dataset: String,
    pub strategy: String,
    pub short_window: Option<usize>,
    pub long_window: Option<usize>,
    pub initial_cash: f64,
    pub final_equity: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
    pub trades: Vec<ApiTrade>,
    pub history: Vec<ApiSnapshot>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiTrade {
    pub order_id: u64,
    pub date: String,
    pub symbol: String,
    pub side: String,
    pub quantity: u64,
    pub price: f64,
    pub fee: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiSnapshot {
    pub date: String,
    pub cash: f64,
    pub position: u64,
    pub close: f64,
    pub equity: f64,
}

impl From<&Trade> for ApiTrade {
    fn from(trade: &Trade) -> Self {
        Self {
            order_id: trade.order_id,
            date: trade.date.clone(),
            symbol: trade.symbol.clone(),
            side: match trade.side {
                Side::Buy => "buy".to_string(),
                Side::Sell => "sell".to_string(),
            },
            quantity: trade.quantity,
            price: trade.price,
            fee: trade.fee,
        }
    }
}

impl From<&PortfolioSnapshot> for ApiSnapshot {
    fn from(snapshot: &PortfolioSnapshot) -> Self {
        Self {
            date: snapshot.date.clone(),
            cash: snapshot.cash,
            position: snapshot.position,
            close: snapshot.close,
            equity: snapshot.equity,
        }
    }
}

pub fn convert_result(
    id: String,
    dataset: String,
    strategy: String,
    short_window: Option<usize>,
    long_window: Option<usize>,
    result: &BacktestResult,
) -> ApiBacktestResult {
    ApiBacktestResult {
        id,
        dataset,
        strategy,
        short_window,
        long_window,
        initial_cash: result.initial_cash,
        final_equity: result.final_equity,
        total_return: result.total_return,
        max_drawdown: result.max_drawdown,
        trades: result.trades.iter().map(ApiTrade::from).collect(),
        history: result.history.iter().map(ApiSnapshot::from).collect(),
    }
}

pub fn make_task_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let sequence = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("bt-{timestamp}-{sequence}")
}