use backtest_core::{BacktestResult, PortfolioSnapshot, Side, Trade};
use serde::{Deserialize, Serialize};

/// 回测执行完成后产生的应用层结果。
///
/// 它与数据库 JSONB 和 HTTP 传输格式解耦；当前字段形状保持旧 API 兼容。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BacktestRunResult {
    pub id: String,
    pub dataset: String,
    pub strategy: String,
    pub short_window: Option<usize>,
    pub long_window: Option<usize>,
    pub initial_cash: f64,
    pub final_equity: f64,
    pub total_return: f64,
    #[serde(default)]
    pub annualized_return: f64,
    pub max_drawdown: f64,
    #[serde(default)]
    pub annualized_volatility: f64,
    #[serde(default)]
    pub sharpe_ratio: f64,
    #[serde(default)]
    pub win_rate: f64,
    #[serde(default)]
    pub profit_factor: f64,
    #[serde(default)]
    pub trade_count: usize,
    #[serde(default)]
    pub closed_trade_count: usize,
    pub trades: Vec<BacktestTradeResult>,
    pub history: Vec<BacktestSnapshotResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BacktestTradeResult {
    pub order_id: u64,
    pub date: String,
    pub symbol: String,
    pub side: String,
    pub quantity: u64,
    pub price: f64,
    pub fee: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BacktestSnapshotResult {
    pub date: String,
    pub cash: f64,
    pub position: u64,
    pub close: f64,
    #[serde(default)]
    pub market_value: f64,
    #[serde(default)]
    pub average_cost: f64,
    #[serde(default)]
    pub unrealized_pnl: f64,
    #[serde(default)]
    pub realized_pnl: f64,
    pub equity: f64,
    #[serde(default)]
    pub daily_return: f64,
    #[serde(default)]
    pub drawdown: f64,
}

impl From<&Trade> for BacktestTradeResult {
    fn from(trade: &Trade) -> Self {
        Self {
            order_id: trade.order_id,
            date: trade.date.clone(),
            symbol: trade.symbol.clone(),
            side: match trade.side {
                Side::Buy => "buy".into(),
                Side::Sell => "sell".into(),
            },
            quantity: trade.quantity,
            price: trade.price,
            fee: trade.fee,
        }
    }
}

impl From<&PortfolioSnapshot> for BacktestSnapshotResult {
    fn from(snapshot: &PortfolioSnapshot) -> Self {
        Self {
            date: snapshot.date.clone(),
            cash: snapshot.cash,
            position: snapshot.position,
            close: snapshot.close,
            market_value: snapshot.market_value,
            average_cost: snapshot.average_cost,
            unrealized_pnl: snapshot.unrealized_pnl,
            realized_pnl: snapshot.realized_pnl,
            equity: snapshot.equity,
            daily_return: snapshot.daily_return,
            drawdown: snapshot.drawdown,
        }
    }
}

impl BacktestRunResult {
    pub fn from_engine(
        id: String,
        dataset: String,
        strategy: String,
        short_window: Option<usize>,
        long_window: Option<usize>,
        result: &BacktestResult,
    ) -> Self {
        Self {
            id,
            dataset,
            strategy,
            short_window,
            long_window,
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
            trades: result.trades.iter().map(BacktestTradeResult::from).collect(),
            history: result.history.iter().map(BacktestSnapshotResult::from).collect(),
        }
    }
}
