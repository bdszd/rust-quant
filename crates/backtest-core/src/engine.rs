use crate::analytics::calculate_full_metrics;
use crate::data::Bar;
use crate::execution::{ExecutionMode, ExecutionModel, Trade};
use crate::portfolio::{Portfolio, PortfolioSnapshot};
use crate::strategy::{Signal, Strategy};

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub initial_cash: f64,
    /// 0.001 表示 0.1%。
    pub commission_rate: f64,
    /// 0.001 表示 0.1%。
    pub slippage_rate: f64,
    /// A 股通常为 100 股一手；设为 1 可用于不限制整手的市场/测试。
    pub lot_size: u64,
    /// 卖出印花税率，买入不收取。
    pub stamp_duty_rate: f64,
    /// 单笔最低佣金。
    pub min_commission: f64,
    pub execution_mode: ExecutionMode,
}

impl BacktestConfig {
    pub fn validate(&self) {
        assert!(
            self.initial_cash.is_finite() && self.initial_cash > 0.0,
            "初始资金必须是有限的正数"
        );
        assert!(
            self.commission_rate.is_finite() && (0.0..1.0).contains(&self.commission_rate),
            "手续费率必须在 [0, 1) 范围内"
        );
        assert!(
            self.slippage_rate.is_finite() && (0.0..1.0).contains(&self.slippage_rate),
            "滑点率必须在 [0, 1) 范围内"
        );
        assert!(self.lot_size > 0, "每手股数必须大于 0");
        assert!(
            self.stamp_duty_rate.is_finite() && (0.0..1.0).contains(&self.stamp_duty_rate),
            "印花税率必须在 [0, 1) 范围内"
        );
        assert!(
            self.min_commission.is_finite() && self.min_commission >= 0.0,
            "最低佣金必须是有限的非负数"
        );
    }
}

#[derive(Debug, Clone)]
pub struct BacktestResult {
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
    pub trades: Vec<Trade>,
    pub history: Vec<PortfolioSnapshot>,
}

/// 每根 Bar 的时序固定为：执行前一根 Bar 的信号 -> 收盘估值 -> 生成下一根 Bar 的信号。
pub fn run<S: Strategy + ?Sized>(
    bars: &[Bar],
    strategy: &mut S,
    config: &BacktestConfig,
) -> BacktestResult {
    config.validate();
    let mut portfolio = Portfolio::new(config.initial_cash);
    let mut execution = ExecutionModel::new(
        config.commission_rate,
        config.slippage_rate,
        config.lot_size,
        config.stamp_duty_rate,
        config.min_commission,
    );
    let mut history = Vec::with_capacity(bars.len());
    let mut pending_signal = Signal::Hold;

    for bar in bars {
        // 新交易日开始：释放上一交易日买入的可卖数量。
        portfolio.begin_day();
        // 只执行上一根 Bar 产生的信号；A 股当日买入后，最早只能在下一交易日卖出。
        let execution_price = match config.execution_mode {
            ExecutionMode::NextOpen => bar.open,
            ExecutionMode::NextClose => bar.close,
        };
        execution
            .execute(
                &mut portfolio,
                pending_signal,
                &bar.symbol,
                &bar.date,
                execution_price,
            )
            .expect("回测成交执行失败");

        history.push(portfolio.snapshot(&bar.date, bar.close));
        pending_signal = strategy.on_bar(bar);
    }

    // 最后一根 Bar 产生的信号没有下一交易日可执行，不在历史数据之外虚构成交。
    let trades = execution.trades().to_vec();
    let metrics = calculate_full_metrics(config.initial_cash, &history, &trades);

    BacktestResult {
        initial_cash: config.initial_cash,
        final_equity: metrics.final_equity,
        total_return: metrics.total_return,
        annualized_return: metrics.annualized_return,
        max_drawdown: metrics.max_drawdown,
        annualized_volatility: metrics.annualized_volatility,
        sharpe_ratio: metrics.sharpe_ratio,
        win_rate: metrics.win_rate,
        profit_factor: metrics.profit_factor,
        trade_count: metrics.trade_count,
        closed_trade_count: metrics.closed_trade_count,
        trades,
        history,
    }
}

/// V2 多股票组合回测结果。
#[derive(Debug, Clone)]
pub struct PortfolioBacktestResult {
    pub initial_cash: f64,
    pub metrics: crate::analytics::PortfolioPerformanceMetrics,
    pub trades: Vec<Trade>,
    pub history: Vec<crate::portfolio::MultiAssetSnapshot>,
    pub benchmark_history: Vec<crate::analytics::BenchmarkSnapshot>,
}

/// 多资产组合回测入口。每个交易日：执行上一交易日的目标权重 -> 收盘估值 -> 生成下一交易日目标权重。
pub fn run_portfolio<S: crate::strategy::PortfolioStrategy>(
    data: &crate::data::MultiAssetData,
    strategy: &mut S,
    config: &BacktestConfig,
    benchmark_bars: Option<&[Bar]>,
) -> Result<PortfolioBacktestResult, String> {
    config.validate();
    let mut portfolio = crate::portfolio::MultiAssetPortfolio::new(config.initial_cash);
    let mut execution = crate::execution::MultiAssetExecutionModel::new(
        config.commission_rate,
        config.slippage_rate,
        config.lot_size,
        config.stamp_duty_rate,
        config.min_commission,
    );
    let mut history = Vec::with_capacity(data.len());
    let mut pending_targets = Vec::new();

    for date in data.calendar() {
        let bars = data
            .bars_on(date)
            .ok_or_else(|| format!("交易日历缺少行情映射：{date}"))?;
        portfolio.begin_day();

        execution.execute_targets(
            &mut portfolio,
            &pending_targets,
            bars,
            date,
            config.execution_mode,
        )?;

        let closes: std::collections::BTreeMap<String, f64> = bars
            .iter()
            .map(|(symbol, bar)| (symbol.clone(), bar.close))
            .collect();
        let snapshot = portfolio.snapshot(date, &closes);
        pending_targets = strategy.on_day(date, bars, &snapshot);
        history.push(snapshot);
    }

    let trades = execution.trades().to_vec();
    let (metrics, benchmark_history) = crate::analytics::calculate_portfolio_metrics(
        config.initial_cash,
        &history,
        &trades,
        benchmark_bars,
    );

    Ok(PortfolioBacktestResult {
        initial_cash: config.initial_cash,
        metrics,
        trades,
        history,
        benchmark_history,
    })
}
