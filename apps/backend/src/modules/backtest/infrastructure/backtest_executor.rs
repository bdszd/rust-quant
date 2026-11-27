use crate::modules::backtest::{
    application::{ports::BacktestExecutor, result::BacktestRunResult},
    domain::{BacktestJobConfig, StrategyConfig},
};
use crate::observability::metrics::Metrics;
use async_trait::async_trait;
use backtest_core::{
    BacktestConfig, Bar, BuyAndHoldStrategy, ExecutionMode as CoreExecutionMode, SmaCrossStrategy,
    Strategy, run,
};
use market_data::{MarketDataRepository, QueryRequest, Symbol};
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

pub fn validate_bars(bars: &[Bar]) -> Result<(), String> {
    if bars.is_empty() {
        return Err("所选数据集、标的和日期范围内没有行情记录".to_string());
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
            return Err(format!("第 {} 条行情数据不合法，请检查 OHLC", index + 1));
        }

        if index > 0 && bars[index - 1].date >= bar.date {
            return Err(format!("第 {} 条行情日期没有严格递增", index + 1));
        }
    }

    Ok(())
}

fn to_engine_bars(rows: Vec<market_data::DailyBar>) -> Result<Vec<Bar>, String> {
    rows.into_iter()
        .map(|row| {
            Ok(Bar {
                symbol: row.symbol.to_string(),
                date: row.trade_date,
                open: row
                    .open
                    .to_f64()
                    .ok_or_else(|| "开盘价无法转换为 f64".to_string())?,
                high: row
                    .high
                    .to_f64()
                    .ok_or_else(|| "最高价无法转换为 f64".to_string())?,
                low: row
                    .low
                    .to_f64()
                    .ok_or_else(|| "最低价无法转换为 f64".to_string())?,
                close: row
                    .close
                    .to_f64()
                    .ok_or_else(|| "收盘价无法转换为 f64".to_string())?,
                volume: row.volume as f64,
                amount: row
                    .amount
                    .map(|value| {
                        value
                            .to_f64()
                            .ok_or_else(|| "成交额无法转换为 f64".to_string())
                    })
                    .transpose()?,
            })
        })
        .collect()
}

fn execute_loaded_bars(
    task_id: Uuid,
    config: BacktestJobConfig,
    bars: Vec<Bar>,
    metrics: Arc<Metrics>,
) -> Result<BacktestRunResult, String> {
    let validate_started = Instant::now();
    validate_bars(&bars)?;
    metrics.record_duration("data_validate", validate_started.elapsed());

    let mut strategy: Box<dyn Strategy> = match &config.strategy {
        StrategyConfig::SmaCross {
            short_window,
            long_window,
        } => Box::new(SmaCrossStrategy::new(*short_window, *long_window))
        StrategyConfig::BuyAndHold => Box::new(BuyAndHoldStrategy::new()),
    };

    let backtest_config = BacktestConfig {
        initial_cash: config.initial_cash,
        commission_rate: config.commission_rate,
        slippage_rate: config.slippage_rate,
        lot_size: config.lot_size,
        stamp_duty_rate: config.stamp_duty_rate,
        min_commission: config.min_commission,
        execution_mode: match config.execution_mode {
            crate::modules::backtest::domain::ExecutionMode::NextOpen => {
                CoreExecutionMode::NextOpen
            }
            crate::modules::backtest::domain::ExecutionMode::NextClose => {
                CoreExecutionMode::NextClose
            }
        },
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

/// 从 PostgreSQL 查询行情；仅将纯 CPU 回测计算放入 spawn_blocking。
pub struct BacktestCoreExecutor {
    market_data: Arc<dyn MarketDataRepository>,
    metrics: Arc<Metrics>,
}

impl BacktestCoreExecutor {
    pub fn new(market_data: Arc<dyn MarketDataRepository>, metrics: Arc<Metrics>) -> Self {
        Self {
            market_data,
            metrics,
        }
    }
}

#[async_trait]
impl BacktestExecutor for BacktestCoreExecutor {
    async fn execute(
        &self,
        task_id: Uuid,
        config: BacktestJobConfig,
    ) -> Result<BacktestRunResult, String> {
        let symbol = Symbol::new(
            config
                .symbols
                .first()
                .ok_or_else(|| "symbols 不能为空".to_string())?
                .clone(),
        )
        .map_err(|error| format!("标的代码无效：{error}"))?;

        let query = QueryRequest {
            dataset_id: config.dataset.clone(),
            symbols: vec![symbol],
            start_date: market_data::normalize::normalize_date(&config.start_date)?,
            end_date: market_data::normalize::normalize_date(&config.end_date)?,
        };

        let load_started = Instant::now();
        let rows = self
            .market_data
            .query_daily_bars(&query)
            .await
            .map_err(|error| format!("从 PostgreSQL 查询行情失败：{error}"))?;
        self.metrics
            .record_duration("data_load", load_started.elapsed());

        let bars = to_engine_bars(rows)?;
        let metrics = self.metrics.clone();
        tokio::task::spawn_blocking(move || execute_loaded_bars(task_id, config, bars, metrics))
            .await
            .map_err(|error| format!("回测计算线程异常：{error}"))?
    }
}
