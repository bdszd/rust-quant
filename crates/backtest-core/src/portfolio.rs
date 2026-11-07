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

/// 单股票账户。只维护资金与持仓，不决定交易信号。
pub struct Portfolio {
    cash: f64,
    position: u64,
    average_cost: f64,
    realized_pnl: f64,
    previous_equity: f64,
    peak_equity: f64,
}

impl Portfolio {
    pub(crate) fn new(initial_cash: f64) -> Self {
        Self {
            cash: initial_cash,
            position: 0,
            average_cost: 0.0,
            realized_pnl: 0.0,
            previous_equity: initial_cash,
            peak_equity: initial_cash,
        }
    }

    pub fn cash(&self) -> f64 { self.cash }
    pub fn position(&self) -> u64 { self.position }
    pub fn average_cost(&self) -> f64 { self.average_cost }
    pub fn realized_pnl(&self) -> f64 { self.realized_pnl }

    /// 成交确认后更新持仓；买入成本包含该笔买入手续费。
    pub(crate) fn apply_buy(&mut self, quantity: u64, price: f64, fee: f64) {
        assert!(quantity > 0 && price.is_finite() && price > 0.0);
        assert!(fee.is_finite() && fee >= 0.0);
        let total_cost = quantity as f64 * price + fee;
        assert!(total_cost <= self.cash + f64::EPSILON, "可用现金不足");

        let old_cost = self.average_cost * self.position as f64;
        self.position += quantity;
        self.average_cost = (old_cost + total_cost) / self.position as f64;
        self.cash -= total_cost;
    }

    /// 卖出时按平均成本法计算已实现盈亏。
    pub(crate) fn apply_sell(&mut self, quantity: u64, price: f64, fee: f64) {
        assert!(quantity > 0 && quantity <= self.position);
        assert!(price.is_finite() && price > 0.0);
        assert!(fee.is_finite() && fee >= 0.0);

        let proceeds = quantity as f64 * price - fee;
        self.realized_pnl += proceeds - self.average_cost * quantity as f64;
        self.cash += proceeds;
        self.position -= quantity;
        if self.position == 0 {
            self.average_cost = 0.0;
        }
    }

    /// 收盘估值，同时更新每日收益和相对历史峰值的回撤。
    pub fn snapshot(&mut self, date: &str, close: f64) -> PortfolioSnapshot {
        assert!(close.is_finite() && close > 0.0, "收盘价必须是有限的正数");
        let market_value = self.position as f64 * close;
        let equity = self.cash + market_value;
        let daily_return = if self.previous_equity > 0.0 {
            equity / self.previous_equity - 1.0
        } else { 0.0 };
        self.peak_equity = self.peak_equity.max(equity);
        let drawdown = if self.peak_equity > 0.0 {
            (self.peak_equity - equity) / self.peak_equity
        } else { 0.0 };
        let unrealized_pnl = if self.position > 0 {
            (close - self.average_cost) * self.position as f64
        } else { 0.0 };
        self.previous_equity = equity;

        PortfolioSnapshot {
            date: date.to_string(),
            cash: self.cash,
            position: self.position,
            close,
            market_value,
            average_cost: self.average_cost,
            unrealized_pnl,
            realized_pnl: self.realized_pnl,
            equity,
            daily_return,
            drawdown,
        }
    }
}
