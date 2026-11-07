use crate::{portfolio::PortfolioSnapshot, Trade};
use trading_core::Side;

#[derive(Debug, Clone, Copy)]
pub struct PerformanceMetrics {
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
}

/// 兼容原接口：只计算由净值序列直接得到的基础指标。
pub fn calculate_metrics(initial_cash: f64, history: &[PortfolioSnapshot]) -> PerformanceMetrics {
    calculate_full_metrics(initial_cash, history, &[])
}

pub fn calculate_full_metrics(
    initial_cash: f64,
    history: &[PortfolioSnapshot],
    trades: &[Trade],
) -> PerformanceMetrics {
    let final_equity = history.last().map(|s| s.equity).unwrap_or(initial_cash);
    let total_return = if initial_cash > 0.0 { final_equity / initial_cash - 1.0 } else { 0.0 };
    let periods = history.len();
    let annualized_return = if periods > 0 && initial_cash > 0.0 && final_equity > 0.0 {
        (final_equity / initial_cash).powf(252.0 / periods as f64) - 1.0
    } else { 0.0 };

    let mut peak = initial_cash;
    let mut max_drawdown: f64 = 0.0;
    for snapshot in history {
        peak = peak.max(snapshot.equity);
        if peak > 0.0 {
            max_drawdown = max_drawdown.max((peak - snapshot.equity) / peak);
        }
    }

    let returns: Vec<f64> = history.iter().map(|s| s.daily_return).collect();
    let mean = if returns.is_empty() { 0.0 } else {
        returns.iter().sum::<f64>() / returns.len() as f64
    };
    let variance = if returns.len() > 1 {
        returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (returns.len() - 1) as f64
    } else { 0.0 };
    let annualized_volatility = variance.sqrt() * 252.0_f64.sqrt();
    let sharpe_ratio = if annualized_volatility > f64::EPSILON {
        mean * 252.0_f64.sqrt() / variance.sqrt()
    } else { 0.0 };

    // 单持仓 V1：按买入与后续卖出配对，手续费计入每笔完整交易盈亏。
    let mut open_lots: Vec<(u64, f64)> = Vec::new();
    let mut closed_pnls = Vec::new();
    for trade in trades {
        match trade.side {
            Side::Buy => open_lots.push((trade.quantity, trade.price * trade.quantity as f64 + trade.fee)),
            Side::Sell => {
                let mut remaining = trade.quantity;
                let mut cost = 0.0;
                while remaining > 0 && !open_lots.is_empty() {
                    let (qty, total_cost) = open_lots.remove(0);
                    let matched = qty.min(remaining);
                    cost += total_cost * matched as f64 / qty as f64;
                    if qty > matched {
                        open_lots.insert(0, (qty - matched, total_cost * (qty - matched) as f64 / qty as f64));
                    }
                    remaining -= matched;
                }
                if remaining == 0 {
                    closed_pnls.push(trade.price * trade.quantity as f64 - trade.fee - cost);
                }
            }
        }
    }
    let winners: f64 = closed_pnls.iter().filter(|p| **p > 0.0).sum();
    let losers: f64 = closed_pnls.iter().filter(|p| **p < 0.0).map(|p| p.abs()).sum();
    let win_rate = if closed_pnls.is_empty() { 0.0 } else {
        closed_pnls.iter().filter(|p| **p > 0.0).count() as f64 / closed_pnls.len() as f64
    };
    let profit_factor = if losers > 0.0 { winners / losers } else if winners > 0.0 { f64::INFINITY } else { 0.0 };

    PerformanceMetrics {
        final_equity, total_return, annualized_return, max_drawdown,
        annualized_volatility, sharpe_ratio, win_rate, profit_factor,
        trade_count: trades.len(), closed_trade_count: closed_pnls.len(),
    }
}
