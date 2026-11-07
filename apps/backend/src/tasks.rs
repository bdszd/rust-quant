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
pub enum TaskStatus { Queued, Running, Succeeded, Failed }

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
    pub annualized_return: f64,
    pub max_drawdown: f64,
    pub annualized_volatility: f64,
    pub sharpe_ratio: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub trade_count: usize,
    pub closed_trade_count: usize,
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
    pub market_value: f64,
    pub average_cost: f64,
    pub unrealized_pnl: f64,
    pub realized_pnl: f64,
    pub equity: f64,
    pub daily_return: f64,
    pub drawdown: f64,
}

impl From<&Trade> for ApiTrade {
    fn from(trade: &Trade) -> Self {
        Self {
            order_id: trade.order_id,
            date: trade.date.clone(),
            symbol: trade.symbol.clone(),
            side: match trade.side { Side::Buy => "buy".into(), Side::Sell => "sell".into() },
            quantity: trade.quantity,
            price: trade.price,
            fee: trade.fee,
        }
    }
}

impl From<&PortfolioSnapshot> for ApiSnapshot {
    fn from(s: &PortfolioSnapshot) -> Self {
        Self {
            date: s.date.clone(), cash: s.cash, position: s.position, close: s.close,
            market_value: s.market_value, average_cost: s.average_cost,
            unrealized_pnl: s.unrealized_pnl, realized_pnl: s.realized_pnl,
            equity: s.equity, daily_return: s.daily_return, drawdown: s.drawdown,
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
        id, dataset, strategy, short_window, long_window,
        initial_cash: result.initial_cash,
        final_equity: result.final_equity,
        total_return: result.total_return,
        annualized_return: result.annualized_return,
        max_drawdown: result.max_drawdown,
        annualized_volatility: result.annualized_volatility,
        sharpe_ratio: result.sharpe_ratio,
        win_rate: result.win_rate,
        profit_factor: result.profit_factor,
        trade_count: result.trade_count,
        closed_trade_count: result.closed_trade_count,
        trades: result.trades.iter().map(ApiTrade::from).collect(),
        history: result.history.iter().map(ApiSnapshot::from).collect(),
    }
}

pub fn make_task_id() -> String {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let sequence = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("bt-{timestamp}-{sequence}")
}
