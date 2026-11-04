
use crate::tasks::{ApiBacktestResult, BacktestJob, convert_result};
use backtest_core::{
    BacktestConfig, Bar, BuyAndHoldStrategy, SmaCrossStrategy, Strategy, load_bars, run,
};
use std::path::Path as FsPath;

pub fn validate_dataset_name(dataset: &str) -> Result<(), String> {
    if dataset.trim().is_empty()
        || dataset.contains('/')
        || dataset.contains('\\')
        || !dataset.ends_with(".csv")
        || FsPath::new(dataset).file_name().and_then(|s| s.to_str()) != Some(dataset)
    {
        return Err("dataset 必须是 data 目录中的 CSV 文件名".to_string());
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

/// 执行实际回测。
///
/// 这个函数由 spawn_blocking 调用，
/// 因此 CSV 读取、数据检查和 run() 不会直接占用 Tokio 异步工作线程。
pub fn execute_job(job: BacktestJob) -> Result<ApiBacktestResult, String> {
    let path = job.data_dir.join(&job.dataset);

    let bars =
        load_bars(path.to_string_lossy().as_ref()).map_err(|e| format!("CSV 读取失败：{e}"))?;

    validate_bars(&bars)?;

    // 根据请求选择具体策略。
    // 创建后统一存放在 Box<dyn Strategy> 中。
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

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(&bars, strategy.as_mut(), &config)
    }))
    .map_err(|_| "回测引擎执行失败，请检查数据与配置".to_string())?;

    Ok(convert_result(
        job.id,
        job.dataset,
        job.strategy,
        job.short_window,
        job.long_window,
        &result,
    ))
}
