pub mod analytics;
pub mod data;
pub mod engine;
pub mod execution;
pub mod indicators;
pub mod portfolio;
pub mod strategy;

pub use data::{Bar, load_bars};
pub use engine::{BacktestConfig, BacktestResult, run};
pub use execution::{ExecutionMode, Trade};
pub use portfolio::PortfolioSnapshot;
pub use strategy::{BuyAndHoldStrategy, Signal, SmaCrossStrategy, Strategy};

pub use trading_core::Side;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::calculate_metrics;
    use crate::execution::ExecutionModel;
    use crate::portfolio::Portfolio;

    fn test_config() -> BacktestConfig {
        BacktestConfig {
            initial_cash: 1000.0,
            commission_rate: 0.001,
            slippage_rate: 0.0,
            execution_mode: ExecutionMode::NextOpen,
        }
    }

    fn make_bar(date: &str, open: f64, close: f64) -> Bar {
        Bar {
            symbol: "TEST".to_string(),
            date: date.to_string(),
            open,
            high: open.max(close) + 0.5,
            low: open.min(close) - 0.5,
            close,
            volume: 1000.0,
            amount: None,
        }
    }

    fn crossover_bars() -> Vec<Bar> {
        vec![
            make_bar("2025-01-01", 10.0, 10.0),
            make_bar("2025-01-02", 9.0, 9.0),
            make_bar("2025-01-03", 8.0, 8.0),
            make_bar("2025-01-04", 9.0, 9.0),
            make_bar("2025-01-05", 11.0, 11.0),
            make_bar("2025-01-06", 12.0, 12.0),
            make_bar("2025-01-07", 10.0, 10.0),
            make_bar("2025-01-08", 8.0, 8.0),
            make_bar("2025-01-09", 7.0, 7.0),
        ]
    }

    #[test]
    fn loads_sample_data() {
        let csv_data = concat!(
            "date,open,high,low,close,volume\n",
            "2025-01-02,240.0,245.0,238.0,243.0,1200000\n",
            "2025-01-03,243.0,248.0,241.0,246.0,1500000\n",
        );

        let mut reader = csv::Reader::from_reader(csv_data.as_bytes());

        let bars: Vec<Bar> = reader.deserialize().collect::<Result<Vec<_>, _>>().unwrap();

        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, 243.0);
        assert_eq!(bars[1].volume, 1_500_000.0);
        assert_eq!(bars[0].symbol, "");
        assert_eq!(bars[0].amount, None);
    }

    #[test]
    fn runs_backtest_without_data() {
        let mut strategy = SmaCrossStrategy::new(2, 3);
        let result = run(&[], &mut strategy, &test_config());

        assert_eq!(result.initial_cash, 1000.0);
        assert_eq!(result.final_equity, 1000.0);
        assert_eq!(result.total_return, 0.0);
        assert_eq!(result.max_drawdown, 0.0);
        assert!(result.trades.is_empty());
        assert!(result.history.is_empty());
    }

    #[test]
    fn buys_and_sells_at_the_next_open_with_fees() {
        let bars = crossover_bars();
        let mut strategy = SmaCrossStrategy::new(2, 3);
        let result = run(&bars, &mut strategy, &test_config());

        assert_eq!(result.trades.len(), 2);

        let buy = &result.trades[0];
        assert_eq!(buy.date, "2025-01-06");
        assert_eq!(buy.side, Side::Buy);
        assert_eq!(buy.price, 12.0);
        assert_eq!(buy.quantity, 83);
        assert!((buy.fee - 0.996).abs() < 1e-9);

        let sell = &result.trades[1];
        assert_eq!(sell.date, "2025-01-09");
        assert_eq!(sell.side, Side::Sell);
        assert_eq!(sell.price, 7.0);
        assert_eq!(sell.quantity, 83);
        assert!((sell.fee - 0.581).abs() < 1e-9);

        let after_buy = &result.history[5];
        assert_eq!(after_buy.position, 83);
        assert!((after_buy.cash - 3.004).abs() < 1e-9);
        assert!((after_buy.equity - 999.004).abs() < 1e-9);

        assert!((result.final_equity - 583.423).abs() < 1e-9);
        assert!((result.total_return - (-0.416577)).abs() < 1e-9);
        assert!((result.max_drawdown - 0.416577).abs() < 1e-9);
    }

    #[test]
    fn zero_commission_preserves_the_original_result() {
        let bars = crossover_bars();
        let mut strategy = SmaCrossStrategy::new(2, 3);

        let config = BacktestConfig {
            initial_cash: 1000.0,
            commission_rate: 0.0,
            slippage_rate: 0.0,
            execution_mode: ExecutionMode::NextOpen,
        };

        let result = run(&bars, &mut strategy, &config);

        assert_eq!(result.trades.len(), 2);
        assert!((result.final_equity - 585.0).abs() < 1e-9);
        assert!((result.total_return - (-0.415)).abs() < 1e-9);
    }

    #[test]
    fn slippage_changes_actual_fill_prices() {
        let mut portfolio = Portfolio::new(1000.0);
        let mut execution = ExecutionModel::new(0.0, 0.01);

        execution
            .execute(&mut portfolio, Signal::Buy, "TEST", "2025-01-01", 100.0)
            .unwrap();

        let buy = &execution.trades()[0];
        assert!((buy.price - 101.0).abs() < 1e-9);
        assert_eq!(buy.quantity, 9);

        execution
            .execute(&mut portfolio, Signal::Sell, "TEST", "2025-01-02", 100.0)
            .unwrap();

        let sell = &execution.trades()[1];
        assert!((sell.price - 99.0).abs() < 1e-9);
        assert_eq!(sell.quantity, 9);
        assert!((portfolio.cash() - 982.0).abs() < 1e-9);
        assert_eq!(portfolio.position(), 0);
    }

    #[test]
    fn execution_modes_use_different_prices() {
        let mut bars = crossover_bars();

        bars[5].open = 13.0;
        bars[5].high = 13.5;

        bars[8].open = 8.0;
        bars[8].high = 8.5;
        bars[8].low = 6.5;

        let open_config = BacktestConfig {
            execution_mode: ExecutionMode::NextOpen,
            ..test_config()
        };

        let close_config = BacktestConfig {
            execution_mode: ExecutionMode::NextClose,
            ..test_config()
        };

        let mut open_strategy = SmaCrossStrategy::new(2, 3);
        let mut close_strategy = SmaCrossStrategy::new(2, 3);

        let open_result = run(&bars, &mut open_strategy, &open_config);
        let close_result = run(&bars, &mut close_strategy, &close_config);

        assert_eq!(open_result.trades.len(), 2);
        assert_eq!(close_result.trades.len(), 2);

        assert_eq!(open_result.trades[0].price, 13.0);
        assert_eq!(open_result.trades[0].quantity, 76);
        assert_eq!(close_result.trades[0].price, 12.0);
        assert_eq!(close_result.trades[0].quantity, 83);

        assert_eq!(open_result.trades[1].price, 8.0);
        assert_eq!(close_result.trades[1].price, 7.0);

        assert!((open_result.final_equity - 618.404).abs() < 1e-9);
        assert!((close_result.final_equity - 583.423).abs() < 1e-9);
    }

    #[test]
    fn analytics_handles_empty_history() {
        let metrics = calculate_metrics(1000.0, &[]);

        assert_eq!(metrics.final_equity, 1000.0);
        assert_eq!(metrics.total_return, 0.0);
        assert_eq!(metrics.max_drawdown, 0.0);
    }

    #[test]
    fn rejects_invalid_commission_rate() {
        let config = BacktestConfig {
            initial_cash: 1000.0,
            commission_rate: -0.01,
            slippage_rate: 0.0,
            execution_mode: ExecutionMode::NextOpen,
        };

        assert!(std::panic::catch_unwind(|| config.validate()).is_err());
    }

    #[test]
    fn rejects_invalid_slippage_rate() {
        let config = BacktestConfig {
            initial_cash: 1000.0,
            commission_rate: 0.001,
            slippage_rate: 1.0,
            execution_mode: ExecutionMode::NextOpen,
        };

        assert!(std::panic::catch_unwind(|| config.validate()).is_err());
    }
}
