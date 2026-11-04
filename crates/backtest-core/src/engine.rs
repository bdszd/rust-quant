
use crate::analytics::calculate_metrics;
use crate::data::Bar;
use crate::execution::{ExecutionMode, ExecutionModel, Trade};
use crate::portfolio::{Portfolio, PortfolioSnapshot};
use crate::strategy::{Signal, Strategy};

/// 一次回测所使用的配置。
#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub initial_cash: f64,
    /// 例如 0.001 表示 0.1%。
    pub commission_rate: f64,
    /// 例如 0.001 表示 0.1%。
    pub slippage_rate: f64,
    pub execution_mode: ExecutionMode,
}

impl BacktestConfig {
    /// 配置的统一校验入口。
    pub fn validate(&self) {
        assert!(
            self.initial_cash.is_finite() && self.initial_cash >= 0.0,
            "初始资金必须是有限的非负数"
        );

        assert!(
            self.commission_rate.is_finite()
                && (0.0..1.0).contains(&self.commission_rate),
            "手续费率必须大于等于 0 且小于 1"
        );

        assert!(
            self.slippage_rate.is_finite()
                && (0.0..1.0).contains(&self.slippage_rate),
            "滑点率必须大于等于 0 且小于 1"
        );
    }
}

#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub initial_cash: f64,
    pub final_equity: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
    pub trades: Vec<Trade>,
    pub history: Vec<PortfolioSnapshot>,
}

/// 执行一次回测。
///
/// Strategy + ?Sized 允许 strategy 是普通的具体策略类型，
/// 也允许它是通过 trait object 使用的 dyn Strategy。
pub fn run<S: Strategy + ?Sized>(
    bars: &[Bar],
    strategy: &mut S,
    config: &BacktestConfig,
) -> BacktestResult {
    // 只在回测入口校验配置。
    config.validate();

    let mut portfolio = Portfolio::new(config.initial_cash);

    let mut execution =
        ExecutionModel::new(config.commission_rate, config.slippage_rate);

    let mut history = Vec::with_capacity(bars.len());
    let mut pending_signal = Signal::Hold;

    
    for bar in bars {
        // 时间点 1：根据执行模式处理上一交易日的信号。
        let execution_price = match config.execution_mode {
            ExecutionMode::NextOpen => bar.open,
            ExecutionMode::NextClose => bar.close,
        };


        execution.execute(
            &mut portfolio,
            pending_signal,
            &bar.date,
            execution_price,
        );

        // 时间点 2：记录当天收盘账户状态。
        history.push(portfolio.snapshot(&bar.date, bar.close));

        // 时间点 3：根据当天行情生成下一交易日的信号。
        pending_signal = strategy.on_bar(bar);
    }

    // 分析模块计算最终资产、收益率和最大回撤。
    let metrics = calculate_metrics(config.initial_cash, &history);

    BacktestResult {
        initial_cash: config.initial_cash,
        final_equity: metrics.final_equity,
        total_return: metrics.total_return,
        max_drawdown: metrics.max_drawdown,
        trades: execution.trades().to_vec(),
        history,
    }
}
