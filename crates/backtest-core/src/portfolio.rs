use std::collections::BTreeMap;
pub use trading_core::Position as AssetPosition;
use trading_core::{Account, Fill};

/// 根据每日组合权益维护收益率和回撤。
///
/// 账户现金、持仓仍由 trading-core::Account 管理；
/// 这里仅保存回测期间的权益序列状态。
#[derive(Debug, Clone)]
struct PerformanceTracker {
    previous_equity: f64,
    peak_equity: f64,
}

impl PerformanceTracker {
    fn new(initial_equity: f64) -> Self {
        Self {
            previous_equity: initial_equity,
            peak_equity: initial_equity,
        }
    }

    fn record(&mut self, equity: f64) -> (f64, f64) {
        let daily_return = if self.previous_equity > 0.0 {
            equity / self.previous_equity - 1.0
        } else {
            0.0
        };

        self.peak_equity = self.peak_equity.max(equity);
        let drawdown = if self.peak_equity > 0.0 {
            (self.peak_equity - equity) / self.peak_equity
        } else {
            0.0
        };

        self.previous_equity = equity;
        (daily_return, drawdown)
    }
}

/// 账户在某个交易日收盘时的完整状态。
#[derive(Debug, Clone)]
pub struct PortfolioSnapshot {
    pub date: String,
    pub cash: f64,
    pub position: u64,
    pub close: f64,
    pub market_value: f64,
    pub average_cost: f64,
    pub unrealized_pnl: f64,
    pub realized_pnl: f64,
    pub equity: f64,
    pub daily_return: f64,
    pub drawdown: f64,
}

/// 单标的回测估值器。
///
/// 账户现金和持仓状态由 trading-core::Account 管理；
/// 此类型额外约束整个回测只能交易一个 symbol，避免用一个 close 对多个标的估值。
pub struct Portfolio {
    account: Account,
    symbol: Option<String>,
    performance: PerformanceTracker,
}

impl Portfolio {
    pub(crate) fn new(initial_cash: f64) -> Self {
        let account = Account::new(initial_cash)
            .expect("BacktestConfig must validate initial_cash before portfolio creation");
        Self {
            account,
            symbol: None,
            performance: PerformanceTracker::new(initial_cash),
        }
    }

    pub fn cash(&self) -> f64 {
        self.account.cash()
    }

    pub fn position(&self) -> u64 {
        self.symbol
            .as_deref()
            .and_then(|symbol| self.account.position(symbol))
            .map(|position| position.quantity)
            .unwrap_or(0)
    }

    pub fn average_cost(&self) -> f64 {
        self.symbol
            .as_deref()
            .and_then(|symbol| self.account.position(symbol))
            .filter(|position| position.quantity > 0)
            .map(|position| position.average_cost)
            .unwrap_or(0.0)
    }

    pub fn realized_pnl(&self) -> f64 {
        self.symbol
            .as_deref()
            .and_then(|symbol| self.account.position(symbol))
            .map(|position| position.realized_pnl)
            .unwrap_or(0.0)
    }

    pub(crate) fn begin_day(&mut self) {
        self.account.begin_day();
    }

    pub(crate) fn apply_fill(&mut self, fill: &Fill) -> Result<(), String> {
        if let Some(symbol) = &self.symbol
            && symbol != &fill.symbol
        {
            return Err(format!(
                "单标的 Portfolio 不允许混合交易：当前标的是 {symbol}，成交标的是 {}",
                fill.symbol
            ));
        }

        self.account.apply_fill(fill).map_err(|e| e.to_string())?;
        if self.symbol.is_none() {
            self.symbol = Some(fill.symbol.clone());
        }
        Ok(())
    }

    /// 收盘估值，同时更新每日收益和相对历史峰值的回撤。
    pub fn snapshot(&mut self, date: &str, close: f64) -> PortfolioSnapshot {
        let position = self
            .symbol
            .as_deref()
            .and_then(|symbol| self.account.position(symbol));
        let quantity = position.map(|p| p.quantity).unwrap_or(0);
        let average_cost = position.map(|p| p.average_cost).unwrap_or(0.0);
        let realized_pnl = position.map(|p| p.realized_pnl).unwrap_or(0.0);
        let cash = self.account.cash();
        let market_value = quantity as f64 * close;
        let equity = cash + market_value;
        let unrealized_pnl = if quantity > 0 {
            (close - average_cost) * quantity as f64
        } else {
            0.0
        };
        let (daily_return, drawdown) = self.performance.record(equity);

        PortfolioSnapshot {
            date: date.to_string(),
            cash,
            position: quantity,
            close,
            market_value,
            average_cost,
            unrealized_pnl,
            realized_pnl,
            equity,
            daily_return,
            drawdown,
        }
    }
}

/// 组合中单只资产的收盘估值。
#[derive(Debug, Clone)]
pub struct AssetPositionSnapshot {
    pub symbol: String,
    pub quantity: u64,
    pub sellable_quantity: u64,
    pub close: Option<f64>,
    pub market_value: f64,
    pub average_cost: f64,
    pub unrealized_pnl: f64,
    pub realized_pnl: f64,
}

/// 多资产组合在某个交易日收盘时的账户快照。
#[derive(Debug, Clone)]
pub struct MultiAssetSnapshot {
    pub date: String,
    pub cash: f64,
    pub positions: BTreeMap<String, AssetPositionSnapshot>,
    pub market_value: f64,
    pub unrealized_pnl: f64,
    pub realized_pnl: f64,
    pub equity: f64,
    pub daily_return: f64,
    pub drawdown: f64,
}

/// 多资产回测估值器。交易账户本身由 trading-core::Account 管理。
pub struct MultiAssetPortfolio {
    account: Account,
    performance: PerformanceTracker,
}

impl MultiAssetPortfolio {
    pub(crate) fn new(initial_cash: f64) -> Self {
        let account = Account::new(initial_cash)
            .expect("BacktestConfig must validate initial_cash before portfolio creation");
        Self {
            account,
            performance: PerformanceTracker::new(initial_cash),
        }
    }

    pub fn cash(&self) -> f64 {
        self.account.cash()
    }

    pub fn positions(&self) -> &BTreeMap<String, AssetPosition> {
        self.account.positions()
    }

    pub fn position(&self, symbol: &str) -> Option<&AssetPosition> {
        self.account.position(symbol)
    }

    pub fn equity(&self) -> f64 {
        self.cash()
            + self
                .positions()
                .values()
                .map(AssetPosition::market_value)
                .sum::<f64>()
    }

    pub(crate) fn begin_day(&mut self) {
        self.account.begin_day();
    }

    pub(crate) fn update_prices(&mut self, prices: &BTreeMap<String, f64>) {
        self.account.update_prices(prices);
    }

    pub(crate) fn apply_fill(&mut self, fill: &Fill) -> Result<(), String> {
        self.account.apply_fill(fill).map_err(|e| e.to_string())
    }

    pub(crate) fn snapshot(
        &mut self,
        date: &str,
        closes: &BTreeMap<String, f64>,
    ) -> MultiAssetSnapshot {
        self.update_prices(closes);
        let positions: BTreeMap<String, AssetPositionSnapshot> = self
            .positions()
            .iter()
            .filter(|(_, p)| p.quantity > 0)
            .map(|(symbol, p)| {
                (
                    symbol.clone(),
                    AssetPositionSnapshot {
                        symbol: symbol.clone(),
                        quantity: p.quantity,
                        sellable_quantity: p.sellable_quantity,
                        close: p.last_price,
                        market_value: p.market_value(),
                        average_cost: p.average_cost,
                        unrealized_pnl: p.unrealized_pnl(),
                        realized_pnl: p.realized_pnl,
                    },
                )
            })
            .collect();
        let market_value = positions.values().map(|p| p.market_value).sum::<f64>();
        // 已平仓标的不会出现在 positions 快照中，但其累计已实现盈亏仍属于账户历史。
        let unrealized_pnl = self
            .positions()
            .values()
            .map(AssetPosition::unrealized_pnl)
            .sum::<f64>();
        let realized_pnl = self
            .positions()
            .values()
            .map(|position| position.realized_pnl)
            .sum::<f64>();
        let cash = self.cash();
        let equity = cash + market_value;
        let (daily_return, drawdown) = self.performance.record(equity);

        MultiAssetSnapshot {
            date: date.to_string(),
            cash,
            positions,
            market_value,
            unrealized_pnl,
            realized_pnl,
            equity,
            daily_return,
            drawdown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MultiAssetPortfolio, Portfolio};
    use std::collections::BTreeMap;
    use trading_core::{Fill, Side};

    fn fill(symbol: &str, side: Side, quantity: u64, price: f64) -> Fill {
        Fill {
            order_id: 1,
            date: "2025-01-02".into(),
            symbol: symbol.into(),
            side,
            quantity,
            price,
            fee: 0.0,
        }
    }

    #[test]
    fn single_asset_portfolio_rejects_a_second_symbol() {
        let mut portfolio = Portfolio::new(10_000.0);
        portfolio
            .apply_fill(&fill("AAA", Side::Buy, 100, 10.0))
            .unwrap();

        let result = portfolio.apply_fill(&fill("BBB", Side::Buy, 100, 20.0));

        assert!(result.is_err());
        assert_eq!(portfolio.position(), 100);
        assert_eq!(portfolio.cash(), 9_000.0);
    }

    #[test]
    fn single_asset_snapshot_uses_consistent_position_and_performance() {
        let mut portfolio = Portfolio::new(10_000.0);
        portfolio
            .apply_fill(&fill("AAA", Side::Buy, 100, 10.0))
            .unwrap();

        let first = portfolio.snapshot("2025-01-02", 11.0);
        let second = portfolio.snapshot("2025-01-03", 9.0);

        assert_eq!(first.position, 100);
        assert_eq!(first.market_value, 1_100.0);
        assert_eq!(first.equity, 10_100.0);
        assert!((first.daily_return - 0.01).abs() < 1e-12);
        assert_eq!(first.drawdown, 0.0);
        assert_eq!(second.equity, 9_900.0);
        assert!(second.daily_return < 0.0);
        assert!((second.drawdown - (200.0 / 10_100.0)).abs() < 1e-12);
    }


    #[test]
    fn multi_asset_snapshot_keeps_realized_pnl_after_position_is_closed() {
        let mut portfolio = MultiAssetPortfolio::new(10_000.0);
        portfolio
            .apply_fill(&fill("AAA", Side::Buy, 100, 10.0))
            .unwrap();
        portfolio.begin_day();
        portfolio
            .apply_fill(&fill("AAA", Side::Sell, 100, 11.0))
            .unwrap();

        let snapshot = portfolio.snapshot("2025-01-03", &BTreeMap::new());

        assert!(snapshot.positions.is_empty());
        assert_eq!(snapshot.market_value, 0.0);
        assert_eq!(snapshot.realized_pnl, 100.0);
        assert_eq!(snapshot.unrealized_pnl, 0.0);
        assert_eq!(snapshot.equity, 10_100.0);
    }

    #[test]
    fn multi_asset_snapshot_updates_prices_and_tracks_performance() {
        let mut portfolio = MultiAssetPortfolio::new(10_000.0);
        portfolio
            .apply_fill(&fill("AAA", Side::Buy, 100, 10.0))
            .unwrap();
        portfolio
            .apply_fill(&fill("BBB", Side::Buy, 100, 20.0))
            .unwrap();

        let first_closes = BTreeMap::from([
            ("AAA".to_string(), 11.0),
            ("BBB".to_string(), 21.0),
        ]);
        let first = portfolio.snapshot("2025-01-02", &first_closes);
        assert_eq!(first.positions.len(), 2);
        assert_eq!(first.market_value, 3_200.0);
        assert!((first.daily_return - 0.02).abs() < 1e-12);

        let second_closes = BTreeMap::from([("AAA".to_string(), 9.0)]);
        let second = portfolio.snapshot("2025-01-03", &second_closes);
        assert_eq!(second.positions["AAA"].close, Some(9.0));
        assert_eq!(second.positions["BBB"].close, Some(21.0));
        assert_eq!(second.market_value, 3_000.0);
        assert!(second.drawdown > 0.0);
    }
}
