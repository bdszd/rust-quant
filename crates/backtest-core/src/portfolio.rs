use std::collections::BTreeMap;
pub use trading_core::Position as AssetPosition;
use trading_core::{Account, Fill};

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

/// 单标的回测估值器。账户现金和持仓状态由 trading-core::Account 管理。
pub struct Portfolio {
    account: Account,
    previous_equity: f64,
    peak_equity: f64,
}

impl Portfolio {
    pub(crate) fn new(initial_cash: f64) -> Self {
        let account = Account::new(initial_cash)
            .expect("BacktestConfig must validate initial_cash before portfolio creation");
        Self {
            account,
            previous_equity: initial_cash,
            peak_equity: initial_cash,
        }
    }

    pub fn cash(&self) -> f64 {
        self.account.cash()
    }

    pub fn position(&self) -> u64 {
        self.account.positions().values().map(|p| p.quantity).sum()
    }

    pub fn average_cost(&self) -> f64 {
        self.account
            .positions()
            .values()
            .find(|p| p.quantity > 0)
            .map(|p| p.average_cost)
            .unwrap_or(0.0)
    }

    pub fn realized_pnl(&self) -> f64 {
        self.account
            .positions()
            .values()
            .map(|p| p.realized_pnl)
            .sum()
    }

    pub(crate) fn begin_day(&mut self) {
        self.account.begin_day();
    }

    pub(crate) fn apply_fill(&mut self, fill: &Fill) -> Result<(), String> {
        self.account.apply_fill(fill).map_err(|e| e.to_string())
    }

    /// 收盘估值，同时更新每日收益和相对历史峰值的回撤。
    pub fn snapshot(&mut self, date: &str, close: f64) -> PortfolioSnapshot {
        assert!(close.is_finite() && close > 0.0, "收盘价必须是有限的正数");
        let position = self.account.positions().values().next();
        let quantity = position.map(|p| p.quantity).unwrap_or(0);
        let average_cost = position.map(|p| p.average_cost).unwrap_or(0.0);
        let realized_pnl = self.realized_pnl();
        let cash = self.account.cash();
        let market_value = quantity as f64 * close;
        let equity = cash + market_value;
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
        let unrealized_pnl = if quantity > 0 {
            (close - average_cost) * quantity as f64
        } else {
            0.0
        };
        self.previous_equity = equity;

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
    pub equity: f64,
    pub daily_return: f64,
    pub drawdown: f64,
}

/// 多资产回测估值器。交易账户本身由 trading-core::Account 管理。
pub struct MultiAssetPortfolio {
    account: Account,
    previous_equity: f64,
    peak_equity: f64,
}

impl MultiAssetPortfolio {
    pub(crate) fn new(initial_cash: f64) -> Self {
        let account = Account::new(initial_cash)
            .expect("BacktestConfig must validate initial_cash before portfolio creation");
        Self {
            account,
            previous_equity: initial_cash,
            peak_equity: initial_cash,
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

    pub(crate) fn update_prices(&mut self, closes: &BTreeMap<String, f64>) {
        self.account.update_prices(closes);
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
        let cash = self.cash();
        let equity = cash + market_value;
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

        MultiAssetSnapshot {
            date: date.to_string(),
            cash,
            positions,
            market_value,
            equity,
            daily_return,
            drawdown,
        }
    }
}
