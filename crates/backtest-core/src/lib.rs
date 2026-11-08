pub mod analytics;

pub use analytics::{BenchmarkSnapshot, PortfolioPerformanceMetrics};
pub mod data;
pub mod engine;
pub mod execution;
pub mod indicators;
pub mod portfolio;
pub mod strategy;

pub use data::{Bar, MultiAssetData, load_bars, load_multi_asset_data};
pub use engine::{BacktestConfig, BacktestResult, PortfolioBacktestResult, run, run_portfolio};
pub use execution::{ExecutionMode, Trade};
pub use portfolio::{AssetPosition, AssetPositionSnapshot, MultiAssetPortfolio, MultiAssetSnapshot, PortfolioSnapshot};
pub use strategy::{BuyAndHoldStrategy, FixedWeightStrategy, PortfolioStrategy, Signal, SmaCrossStrategy, Strategy, TargetWeight};

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
            lot_size: 1,
            stamp_duty_rate: 0.0,
            min_commission: 0.0,
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
            lot_size: 1,
            stamp_duty_rate: 0.0,
            min_commission: 0.0,
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
        let mut execution = ExecutionModel::new(0.0, 0.01, 1, 0.0, 0.0);

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

    struct BuyThenSell {
        bars_seen: usize,
    }

    impl Strategy for BuyThenSell {
        fn on_bar(&mut self, _bar: &Bar) -> Signal {
            self.bars_seen += 1;
            match self.bars_seen {
                1 => Signal::Buy,
                2 => Signal::Sell,
                _ => Signal::Hold,
            }
        }
    }

    #[test]
    fn a_share_buy_cannot_be_sold_on_the_same_trading_day() {
        let bars = vec![
            make_bar("2025-01-02", 10.0, 10.0),
            make_bar("2025-01-03", 10.0, 10.0),
            make_bar("2025-01-06", 11.0, 11.0),
        ];
        let mut strategy = BuyThenSell { bars_seen: 0 };
        let config = BacktestConfig {
            initial_cash: 10_000.0,
            commission_rate: 0.0,
            slippage_rate: 0.0,
            lot_size: 100,
            stamp_duty_rate: 0.0,
            min_commission: 0.0,
            execution_mode: ExecutionMode::NextOpen,
        };

        let result = run(&bars, &mut strategy, &config);

        assert_eq!(result.trades.len(), 2);
        assert_eq!(result.trades[0].date, "2025-01-03");
        assert_eq!(result.trades[0].side, Side::Buy);
        assert_eq!(result.trades[1].date, "2025-01-06");
        assert_eq!(result.trades[1].side, Side::Sell);
    }

    #[test]
    fn applies_a_share_lot_minimum_commission_and_sell_stamp_duty() {
        let mut portfolio = Portfolio::new(10_000.0);
        let mut execution = ExecutionModel::new(0.0003, 0.0, 100, 0.0005, 5.0);

        execution
            .execute(&mut portfolio, Signal::Buy, "000001.SZ", "2025-01-02", 10.0)
            .unwrap();

        let buy = &execution.trades()[0];
        assert_eq!(buy.quantity, 900);
        assert!((buy.fee - 5.0).abs() < 1e-9);
        assert!((portfolio.cash() - 995.0).abs() < 1e-9);

        execution
            .execute(
                &mut portfolio,
                Signal::Sell,
                "000001.SZ",
                "2025-01-03",
                11.0,
            )
            .unwrap();

        let sell = &execution.trades()[1];
        assert_eq!(sell.quantity, 900);
        assert!((sell.fee - 9.95).abs() < 1e-9);
        assert!((portfolio.cash() - 10_885.05).abs() < 1e-9);
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
            lot_size: 1,
            stamp_duty_rate: 0.0,
            min_commission: 0.0,
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
            lot_size: 1,
            stamp_duty_rate: 0.0,
            min_commission: 0.0,
            execution_mode: ExecutionMode::NextOpen,
        };

        assert!(std::panic::catch_unwind(|| config.validate()).is_err());
    }

    fn make_symbol_bar(symbol: &str, date: &str, open: f64, close: f64) -> Bar {
        Bar {
            symbol: symbol.to_string(),
            date: date.to_string(),
            open,
            high: open.max(close) + 0.5,
            low: open.min(close) - 0.5,
            close,
            volume: 10_000.0,
            amount: None,
        }
    }

    #[test]
    fn multi_asset_data_builds_union_calendar_without_filling_missing_bars() {
        let data = MultiAssetData::new(vec![
            make_symbol_bar("AAA", "2025-01-02", 10.0, 10.0),
            make_symbol_bar("AAA", "2025-01-03", 11.0, 11.0),
            make_symbol_bar("BBB", "2025-01-02", 20.0, 20.0),
            make_symbol_bar("BBB", "2025-01-06", 21.0, 21.0),
        ]).unwrap();

        assert_eq!(data.calendar(), &vec!["2025-01-02".to_string(), "2025-01-03".to_string(), "2025-01-06".to_string()]);
        assert_eq!(data.symbols(), &vec!["AAA".to_string(), "BBB".to_string()]);
        assert_eq!(data.bars_on("2025-01-03").unwrap().len(), 1);
        assert!(!data.bars_on("2025-01-03").unwrap().contains_key("BBB"));
    }

    #[test]
    fn portfolio_backtest_tracks_multiple_positions_and_cash() {
        let data = MultiAssetData::new(vec![
            make_symbol_bar("AAA", "2025-01-02", 10.0, 10.0),
            make_symbol_bar("BBB", "2025-01-02", 20.0, 20.0),
            make_symbol_bar("AAA", "2025-01-03", 11.0, 11.0),
            make_symbol_bar("BBB", "2025-01-03", 20.0, 20.0),
            make_symbol_bar("AAA", "2025-01-06", 12.0, 12.0),
            make_symbol_bar("BBB", "2025-01-06", 22.0, 22.0),
        ]).unwrap();
        let mut strategy = FixedWeightStrategy::new(vec![
            TargetWeight { symbol: "AAA".into(), weight: 0.4 },
            TargetWeight { symbol: "BBB".into(), weight: 0.4 },
        ]);
        let config = BacktestConfig {
            initial_cash: 100_000.0,
            commission_rate: 0.0,
            slippage_rate: 0.0,
            lot_size: 100,
            stamp_duty_rate: 0.0,
            min_commission: 0.0,
            execution_mode: ExecutionMode::NextOpen,
        };

        let result = run_portfolio(&data, &mut strategy, &config, None).unwrap();
        assert_eq!(result.trades.len(), 2);
        assert_eq!(result.trades[0].symbol, "AAA");
        assert_eq!(result.trades[1].symbol, "BBB");
        assert!(result.history[1].positions.contains_key("AAA"));
        assert!(result.history[1].positions.contains_key("BBB"));
        assert!(result.history[1].cash > 0.0);
        assert!(result.metrics.final_equity > config.initial_cash);
    }

    struct BuyThenExit {
        day: usize,
    }

    impl PortfolioStrategy for BuyThenExit {
        fn on_day(
            &mut self,
            _date: &str,
            _bars: &std::collections::BTreeMap<String, Bar>,
            _portfolio: &MultiAssetSnapshot,
        ) -> Vec<TargetWeight> {
            self.day += 1;
            vec![TargetWeight {
                symbol: "AAA".into(),
                weight: if self.day == 1 { 0.5 } else { 0.0 },
            }]
        }
    }

    #[test]
    fn portfolio_respects_t_plus_one_and_waits_for_symbol_bar() {
        let data = MultiAssetData::new(vec![
            make_symbol_bar("AAA", "2025-01-02", 10.0, 10.0),
            make_symbol_bar("AAA", "2025-01-03", 11.0, 11.0),
            make_symbol_bar("AAA", "2025-01-06", 12.0, 12.0),
            make_symbol_bar("BBB", "2025-01-03", 20.0, 20.0),
        ]).unwrap();
        let mut strategy = BuyThenExit { day: 0 };
        let config = BacktestConfig {
            initial_cash: 100_000.0,
            commission_rate: 0.0,
            slippage_rate: 0.0,
            lot_size: 100,
            stamp_duty_rate: 0.0,
            min_commission: 0.0,
            execution_mode: ExecutionMode::NextOpen,
        };

        let result = run_portfolio(&data, &mut strategy, &config, None).unwrap();
        assert_eq!(result.trades.len(), 2);
        assert_eq!(result.trades[0].date, "2025-01-03");
        assert_eq!(result.trades[0].side, Side::Buy);
        assert_eq!(result.trades[1].date, "2025-01-06");
        assert_eq!(result.trades[1].side, Side::Sell);
    }

    #[test]
    fn portfolio_benchmark_comparison_produces_aligned_history() {
        let data = MultiAssetData::new(vec![
            make_symbol_bar("AAA", "2025-01-02", 10.0, 10.0),
            make_symbol_bar("AAA", "2025-01-03", 10.0, 11.0),
            make_symbol_bar("AAA", "2025-01-06", 10.0, 12.0),
        ]).unwrap();
        let benchmark = vec![
            make_symbol_bar("INDEX", "2025-01-02", 100.0, 100.0),
            make_symbol_bar("INDEX", "2025-01-03", 101.0, 101.0),
            make_symbol_bar("INDEX", "2025-01-06", 102.0, 102.0),
        ];
        let mut strategy = FixedWeightStrategy::new(vec![
            TargetWeight { symbol: "AAA".into(), weight: 0.5 },
        ]);
        let config = BacktestConfig {
            initial_cash: 100_000.0,
            commission_rate: 0.0,
            slippage_rate: 0.0,
            lot_size: 100,
            stamp_duty_rate: 0.0,
            min_commission: 0.0,
            execution_mode: ExecutionMode::NextOpen,
        };

        let result = run_portfolio(&data, &mut strategy, &config, Some(&benchmark)).unwrap();
        assert_eq!(result.benchmark_history.len(), data.len());
        assert!(result.metrics.benchmark_return.is_some());
        assert!(result.metrics.excess_return.is_some());
    }

}
