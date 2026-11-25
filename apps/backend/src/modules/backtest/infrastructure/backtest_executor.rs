use crate::modules::backtest::{
    application::{ports::BacktestExecutor, result::BacktestRunResult},
    domain::{BacktestJobConfig, StrategyConfig},
};
use crate::observability::metrics::Metrics;
use backtest_core::{
    BacktestConfig, Bar, BuyAndHoldStrategy, SmaCrossStrategy, Strategy, load_data, run,
};
use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Instant,
};
use uuid::Uuid;

pub fn validate_bars(bars: &[Bar]) -> Result<(), String> {
    if bars.is_empty() {
        return Err("数据集没有行情记录".to_string());
    }

    for (index, bar) in bars.iter().enumerate() {
        let valid = !bar.date.trim().is_empty()
            && bar.open.is_finite()
            && bar.high.is_finite()
            && bar.low.is_finite()
            && bar.close.is_finite()
            && bar.open > 0.0
            && bar.high > 0.0
            && bar.low > 0.0
            && bar.close > 0.0
            && bar.low <= bar.open.min(bar.close)
            && bar.high >= bar.open.max(bar.close);

        if !valid {
            return Err(format!("第 {} 行行情数据不合法，请检查 OHLC", index + 2));
        }

        if index > 0 && bars[index - 1].date >= bar.date {
            return Err(format!("第 {} 行日期没有严格递增", index + 2));
        }
    }

    Ok(())
}

/// 根据持久化的不可变配置执行回测。
///
/// data_dir 是当前 Backend 进程的运行环境配置，不写入 Job，也不从 RabbitMQ 消息读取。
pub fn execute_config(
    task_id: uuid::Uuid,
    config: BacktestJobConfig,
    data_dir: &FsPath,
    metrics: Arc<Metrics>,
) -> Result<BacktestRunResult, String> {
    let path = data_dir.join(&config.dataset).join("bars.csv");

    let load_started = Instant::now();
    let bars = load_data(path.to_string_lossy().as_ref())
        .map_err(|error| format!("行情 CSV 读取失败：{error}"))?;
    metrics.record_duration("data_load", load_started.elapsed());

    let validate_started = Instant::now();
    validate_bars(&bars)?;
    metrics.record_duration("data_validate", validate_started.elapsed());

    let mut strategy: Box<dyn Strategy> = match &config.strategy {
        StrategyConfig::SmaCross {
            short_window,
            long_window,
        } => Box::new(SmaCrossStrategy::new(*short_window, *long_window)),
        StrategyConfig::BuyAndHold => Box::new(BuyAndHoldStrategy::new()),
    };

    let backtest_config = BacktestConfig {
        initial_cash: config.initial_cash,
        commission_rate: config.commission_rate,
        slippage_rate: config.slippage_rate,
        lot_size: config.lot_size,
        stamp_duty_rate: config.stamp_duty_rate,
        min_commission: config.min_commission,
        execution_mode: config.execution_mode,
    };

    let engine_started = Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(&bars, strategy.as_mut(), &backtest_config)
    }))
    .map_err(|_| "回测引擎执行失败，请检查数据与配置".to_string())?;
    metrics.record_duration("engine_execute", engine_started.elapsed());

    let (strategy_name, short_window, long_window) = match config.strategy {
        StrategyConfig::SmaCross {
            short_window,
            long_window,
        } => (
            "sma_cross".to_string(),
            Some(short_window),
            Some(long_window),
        ),
        StrategyConfig::BuyAndHold => ("buy_and_hold".to_string(), None, None),
    };

    Ok(BacktestRunResult::from_engine(
        task_id.to_string(),
        config.dataset,
        strategy_name,
        short_window,
        long_window,
        &result,
    ))
}

/// Backtest Core 的执行适配器。
///
/// 负责把文件数据集、运行指标和回测引擎组合起来。
/// Worker 只依赖 BacktestExecutor，不直接接触这些实现细节。
pub struct BacktestCoreExecutor {
    data_dir: PathBuf,
    metrics: Arc<Metrics>,
}

impl BacktestCoreExecutor {
    pub fn new(data_dir: PathBuf, metrics: Arc<Metrics>) -> Self {
        Self { data_dir, metrics }
    }
}

impl BacktestExecutor for BacktestCoreExecutor {
    fn execute(
        &self,
        task_id: Uuid,
        config: BacktestJobConfig,
    ) -> Result<BacktestRunResult, String> {
        execute_config(task_id, config, &self.data_dir, self.metrics.clone())
    }
}
