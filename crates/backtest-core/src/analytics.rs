use crate::portfolio::PortfolioSnapshot;

#[derive(Debug, Clone, Copy)]
pub struct PerformanceMetrics {
    pub final_equity: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
}

/// 根据每日账户快照计算回测绩效。
pub fn calculate_metrics(initial_cash: f64, history: &[PortfolioSnapshot]) -> PerformanceMetrics {
    let final_equity = history
        .last()
        .map(|snapshot| snapshot.equity)
        .unwrap_or(initial_cash);

    let total_return = if initial_cash > 0.0 {
        final_equity / initial_cash - 1.0
    } else {
        0.0
    };

    // 使用初始资金作为第一个峰值基准。
    let mut peak = initial_cash;
    let mut max_drawdown: f64 = 0.0;

    for snapshot in history {
        peak = peak.max(snapshot.equity);

        if peak > 0.0 {
            let drawdown = (peak - snapshot.equity) / peak;
            max_drawdown = max_drawdown.max(drawdown);
        }
    }

    PerformanceMetrics {
        final_equity,
        total_return,
        max_drawdown,
    }
}
