/// 账户在某个交易日收盘时的状态。
#[derive(Debug, Clone)]
pub struct PortfolioSnapshot {
    pub date: String,
    pub cash: f64,
    pub position: u64,
    pub close: f64,
    pub equity: f64,
}

/// 只负责维护账户状态，不负责决定何时成交。
pub struct Portfolio {
    cash: f64,
    position: u64,
}

impl Portfolio {
    pub(crate) fn new(initial_cash: f64) -> Self {
        Self {
            cash: initial_cash,
            position: 0,
        }
    }

    pub fn cash(&self) -> f64 {
        self.cash
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    /// 执行模块确认买入成交后，更新账户。
    pub(crate) fn apply_buy(&mut self, quantity: u64, price: f64, fee: f64) {
        assert!(self.position == 0);
        assert!(quantity > 0);
        assert!(price.is_finite() && price > 0.0);
        assert!(fee.is_finite() && fee >= 0.0);

        let cost = quantity as f64 * price + fee;
        assert!(cost <= self.cash);

        self.cash -= cost;
        self.position = quantity;
    }

    /// 执行模块确认卖出成交后，更新账户。
    pub(crate) fn apply_sell(&mut self, quantity: u64, price: f64, fee: f64) {
        assert!(quantity > 0);
        assert_eq!(quantity, self.position);
        assert!(price.is_finite() && price > 0.0);
        assert!(fee.is_finite() && fee >= 0.0);

        let proceeds = quantity as f64 * price - fee;

        self.cash += proceeds;
        self.position = 0;
    }

    /// 按市场收盘价估算账户资产，不产生交易。
    pub fn snapshot(&self, date: &str, close: f64) -> PortfolioSnapshot {
        assert!(close.is_finite() && close > 0.0, "收盘价必须是有限的正数");

        let equity = self.cash + self.position as f64 * close;

        PortfolioSnapshot {
            date: date.to_string(),
            cash: self.cash,
            position: self.position,
            close,
            equity,
        }
    }
}
