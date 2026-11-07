use crate::metrics::Metrics;
use crate::tasks::{ApiBacktestResult, BacktestJob, convert_result};
use backtest_core::{
    BacktestConfig, Bar, BuyAndHoldStrategy, SmaCrossStrategy, Strategy, load_bars, run,
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
    let bars = load_bars(path.to_string_lossy().as_ref())
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
