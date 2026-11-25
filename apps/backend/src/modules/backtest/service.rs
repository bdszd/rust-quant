use crate::observability::metrics::Metrics;
use crate::tasks::{ApiBacktestResult, BacktestJob, convert_result};
use backtest_core::{
    BacktestConfig, Bar, BuyAndHoldStrategy, SmaCrossStrategy, Strategy, load_data, run,
};
use std::{path::Path as FsPath, sync::Arc, time::Instant};

pub fn validate_dataset_name(dataset: &str) -> Result<(), String> {
    let mut parts = dataset.split('/');

    let Some(symbol) = parts.next() else {
        return Err("dataset 必须使用 股票代码/日期范围 格式".to_string());
    };
    let Some(date_range) = parts.next() else {
        return Err("dataset 必须使用 股票代码/日期范围 格式".to_string());
    };

    if parts.next().is_some()
        || symbol.trim().is_empty()
        || date_range.trim().is_empty()
        || FsPath::new(symbol).file_name().and_then(|s| s.to_str()) != Some(symbol)
        || FsPath::new(date_range).file_name().and_then(|s| s.to_str()) != Some(date_range)
        || symbol.contains('\\')
        || date_range.contains('\\')
    {
        return Err("dataset 必须使用有效的 股票代码/日期范围 格式".to_string());
    }

    Ok(())
}

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

/// 执行实际回测。行情来自 data/cleaned/<symbol>/<date-range>/bars.csv。
pub fn execute_job(job: BacktestJob, metrics: Arc<Metrics>) -> Result<ApiBacktestResult, String> {
    let path = job.data_dir.join(&job.dataset).join("bars.csv");

    let load_started = Instant::now();
    let bars = load_data(path.to_string_lossy().as_ref())
        .map_err(|e| format!("行情 CSV 读取失败：{e}"))?;
    metrics.record_duration("data_load", load_started.elapsed());

    let validate_started = Instant::now();
    validate_bars(&bars)?;
    metrics.record_duration("data_validate", validate_started.elapsed());

    let mut strategy: Box<dyn Strategy> = match job.strategy.as_str() {
        "sma_cross" => Box::new(SmaCrossStrategy::new(
            job.short_window.unwrap_or(2),
            job.long_window.unwrap_or(3),
        )),
        "buy_and_hold" => Box::new(BuyAndHoldStrategy::new()),
        _ => return Err("内部错误：不支持的策略类型".to_string()),
    };

    let config = BacktestConfig {
        initial_cash: job.initial_cash,
        commission_rate: job.commission_rate,
        slippage_rate: job.slippage_rate,
        lot_size: job.lot_size,
        stamp_duty_rate: job.stamp_duty_rate,
        min_commission: job.min_commission,
        execution_mode: job.execution_mode,
    };

    let engine_started = Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(&bars, strategy.as_mut(), &config)
    }))
    .map_err(|_| "回测引擎执行失败，请检查数据与配置".to_string())?;
    metrics.record_duration("engine_execute", engine_started.elapsed());

    Ok(convert_result(
        job.id,
        job.dataset,
        job.strategy,
        job.short_window,
        job.long_window,
        &result,
    ))
}


/// 根据持久化的不可变配置执行回测。
///
/// data_dir 是当前 Backend 进程的运行环境配置，不写入 Job，也不从 RabbitMQ 消息读取。
pub fn execute_config(
    task_id: uuid::Uuid,
    config: crate::modules::backtest::domain::BacktestJobConfig,
    data_dir: &FsPath,
    metrics: Arc<Metrics>,
) -> Result<ApiBacktestResult, String> {
    use crate::modules::backtest::domain::StrategyConfig;

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
        } => ("sma_cross".to_string(), Some(short_window), Some(long_window)),
        StrategyConfig::BuyAndHold => ("buy_and_hold".to_string(), None, None),
    };

    Ok(convert_result(
        task_id.to_string(),
        config.dataset,
        strategy_name,
        short_window,
        long_window,
        &result,
    ))
}
