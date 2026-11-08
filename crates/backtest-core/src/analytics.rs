use crate::{Trade, portfolio::PortfolioSnapshot};
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
    let total_return = if initial_cash > 0.0 {
        final_equity / initial_cash - 1.0
    } else {
        0.0
    };
    let periods = history.len();
    let annualized_return = if periods > 0 && initial_cash > 0.0 && final_equity > 0.0 {
        (final_equity / initial_cash).powf(252.0 / periods as f64) - 1.0
    } else {
        0.0
    };

    let mut peak = initial_cash;
    let mut max_drawdown: f64 = 0.0;
    for snapshot in history {
        peak = peak.max(snapshot.equity);
        if peak > 0.0 {
            max_drawdown = max_drawdown.max((peak - snapshot.equity) / peak);
        }
    }

    let returns: Vec<f64> = history.iter().map(|s| s.daily_return).collect();
    let mean = if returns.is_empty() {
        0.0
    } else {
        returns.iter().sum::<f64>() / returns.len() as f64
    };
    let variance = if returns.len() > 1 {
        returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (returns.len() - 1) as f64
    } else {
        0.0
    };
    let annualized_volatility = variance.sqrt() * 252.0_f64.sqrt();
    let sharpe_ratio = if annualized_volatility > f64::EPSILON {
        mean * 252.0_f64.sqrt() / variance.sqrt()
    } else {
        0.0
    };

    // 单持仓 V1：按买入与后续卖出配对，手续费计入每笔完整交易盈亏。
    let mut open_lots: Vec<(u64, f64)> = Vec::new();
    let mut closed_pnls = Vec::new();
    for trade in trades {
        match trade.side {
            Side::Buy => open_lots.push((
                trade.quantity,
                trade.price * trade.quantity as f64 + trade.fee,
            )),
            Side::Sell => {
                let mut remaining = trade.quantity;
                let mut cost = 0.0;
                while remaining > 0 && !open_lots.is_empty() {
                    let (qty, total_cost) = open_lots.remove(0);
                    let matched = qty.min(remaining);
                    cost += total_cost * matched as f64 / qty as f64;
                    if qty > matched {
                        open_lots.insert(
                            0,
                            (
                                qty - matched,
                                total_cost * (qty - matched) as f64 / qty as f64,
                            ),
                        );
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
    let losers: f64 = closed_pnls
        .iter()
        .filter(|p| **p < 0.0)
        .map(|p| p.abs())
        .sum();
    let win_rate = if closed_pnls.is_empty() {
        0.0
    } else {
        closed_pnls.iter().filter(|p| **p > 0.0).count() as f64 / closed_pnls.len() as f64
    };
    let profit_factor = if losers > 0.0 {
        winners / losers
    } else if winners > 0.0 {
        999.0
    } else {
        0.0
    };

    PerformanceMetrics {
        final_equity,
        total_return,
        annualized_return,
        max_drawdown,
        annualized_volatility,
        sharpe_ratio,
        win_rate,
        profit_factor,
        trade_count: trades.len(),
        closed_trade_count: closed_pnls.len(),
    }
}


/// 基准净值在组合交易日历上的估值。
#[derive(Debug, Clone)]
pub struct BenchmarkSnapshot {
    pub date: String,
    pub equity: f64,
    pub daily_return: f64,
}

/// V2 组合绩效指标。
#[derive(Debug, Clone, Copy)]
pub struct PortfolioPerformanceMetrics {
    pub final_equity: f64,
    pub total_return: f64,
    pub annualized_return: f64,
    pub max_drawdown: f64,
    pub annualized_volatility: f64,
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,
    pub benchmark_return: Option<f64>,
    pub excess_return: Option<f64>,
    pub cumulative_turnover_ratio: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub trade_count: usize,
    pub closed_trade_count: usize,
}

/// 根据组合净值计算绩效。基准缺少某个组合交易日时，只沿用此前已知收盘价，
/// 不使用未来价格；这仅用于基准估值，不会填充个股交易行情。
pub fn calculate_portfolio_metrics(
    initial_cash: f64,
    history: &[crate::portfolio::MultiAssetSnapshot],
    trades: &[Trade],
    benchmark_bars: Option<&[crate::data::Bar]>,
) -> (PortfolioPerformanceMetrics, Vec<BenchmarkSnapshot>) {
    use std::collections::BTreeMap;

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
    let sharpe_ratio = if variance > f64::EPSILON {
        mean / variance.sqrt() * 252.0_f64.sqrt()
    } else { 0.0 };
    let downside_variance = if returns.len() > 1 {
        returns.iter().map(|r| r.min(0.0).powi(2)).sum::<f64>() / (returns.len() - 1) as f64
    } else { 0.0 };
    let sortino_ratio = if downside_variance > f64::EPSILON {
        mean / downside_variance.sqrt() * 252.0_f64.sqrt()
    } else { 0.0 };
    let calmar_ratio = if max_drawdown > f64::EPSILON {
        annualized_return / max_drawdown
    } else { 0.0 };

    // 按 symbol 独立进行 FIFO 成本配对，绝不跨股票匹配买卖成交。
    let mut open_positions: BTreeMap<String, (u64, f64)> = BTreeMap::new();
    let mut closed_pnls = Vec::new();
    for trade in trades {
        let entry = open_positions.entry(trade.symbol.clone()).or_insert((0, 0.0));
        match trade.side {
            Side::Buy => {
                entry.0 += trade.quantity;
                entry.1 += trade.price * trade.quantity as f64 + trade.fee;
            }
            Side::Sell => {
                let matched = trade.quantity.min(entry.0);
                if matched == 0 { continue; }
                let average_cost = entry.1 / entry.0 as f64;
                let matched_cost = average_cost * matched as f64;
                entry.0 -= matched;
                entry.1 -= matched_cost;
                if matched == trade.quantity {
                    closed_pnls.push(trade.price * matched as f64 - trade.fee - matched_cost);
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

    let traded_value: f64 = trades.iter().map(|t| t.price * t.quantity as f64).sum();
    let average_equity = if history.is_empty() { initial_cash } else {
        history.iter().map(|s| s.equity).sum::<f64>() / history.len() as f64
    };
    let cumulative_turnover_ratio = if average_equity > 0.0 { traded_value / average_equity } else { 0.0 };

    let mut benchmark_history = Vec::new();
    let benchmark_return = benchmark_bars.and_then(|bars| {
        if bars.is_empty() { return None; }
        let mut by_date = BTreeMap::new();
        for bar in bars {
            by_date.insert(bar.date.clone(), bar.close);
        }
        let first_date = &history.first()?.date;
        let first_close = *by_date.range(..=first_date.clone()).next_back()?.1;
        let units = initial_cash / first_close;
        let mut last_close = first_close;
        let mut previous_equity = initial_cash;
        for snapshot in history {
            if let Some(close) = by_date.get(&snapshot.date) {
                last_close = *close;
            }
            let equity = units * last_close;
            let daily_return = if previous_equity > 0.0 { equity / previous_equity - 1.0 } else { 0.0 };
            benchmark_history.push(BenchmarkSnapshot {
                date: snapshot.date.clone(), equity, daily_return,
            });
            previous_equity = equity;
        }
        benchmark_history.last().map(|s| s.equity / initial_cash - 1.0)
    });
    let excess_return = benchmark_return.map(|r| total_return - r);

    (
        PortfolioPerformanceMetrics {
            final_equity, total_return, annualized_return, max_drawdown,
            annualized_volatility, sharpe_ratio, sortino_ratio, calmar_ratio,
            benchmark_return, excess_return, cumulative_turnover_ratio,
            win_rate, profit_factor, trade_count: trades.len(),
            closed_trade_count: closed_pnls.len(),
        },
        benchmark_history,
    )
}
