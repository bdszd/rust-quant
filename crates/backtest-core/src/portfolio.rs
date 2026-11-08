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

    pub fn cash(&self) -> f64 {
        self.cash
    }
    pub fn position(&self) -> u64 {
        self.position
    }
    pub fn average_cost(&self) -> f64 {
        self.average_cost
    }
    pub fn realized_pnl(&self) -> f64 {
        self.realized_pnl
    }

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
        } else {
            0.0
        };
        self.peak_equity = self.peak_equity.max(equity);
        let drawdown = if self.peak_equity > 0.0 {
            (self.peak_equity - equity) / self.peak_equity
        } else {
            0.0
        };
        let unrealized_pnl = if self.position > 0 {
            (close - self.average_cost) * self.position as f64
        } else {
            0.0
        };
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


/// 单只股票在组合账户中的持仓状态。
#[derive(Debug, Clone)]
pub struct AssetPosition {
    pub symbol: String,
    pub quantity: u64,
    pub sellable_quantity: u64,
    pub average_cost: f64,
    pub last_price: Option<f64>,
    pub realized_pnl: f64,
    pub bought_today: u64,
}

impl AssetPosition {
    fn new(symbol: String) -> Self {
        Self {
            symbol,
            quantity: 0,
            sellable_quantity: 0,
            average_cost: 0.0,
            last_price: None,
            realized_pnl: 0.0,
            bought_today: 0,
        }
    }

    pub fn market_value(&self) -> f64 {
        self.last_price.unwrap_or(0.0) * self.quantity as f64
    }

    pub fn unrealized_pnl(&self) -> f64 {
        (self.last_price.unwrap_or(self.average_cost) - self.average_cost) * self.quantity as f64
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
    pub positions: std::collections::BTreeMap<String, AssetPositionSnapshot>,
    pub market_value: f64,
    pub equity: f64,
    pub daily_return: f64,
    pub drawdown: f64,
}

/// V2 组合账户。仍暂时放在 backtest-core，后续由用户核查边界后再决定迁移。
pub struct MultiAssetPortfolio {
    cash: f64,
    positions: std::collections::BTreeMap<String, AssetPosition>,
    previous_equity: f64,
    peak_equity: f64,
}

impl MultiAssetPortfolio {
    pub(crate) fn new(initial_cash: f64) -> Self {
        Self {
            cash: initial_cash,
            positions: std::collections::BTreeMap::new(),
            previous_equity: initial_cash,
            peak_equity: initial_cash,
        }
    }

    pub fn cash(&self) -> f64 {
        self.cash
    }

    pub fn positions(&self) -> &std::collections::BTreeMap<String, AssetPosition> {
        &self.positions
    }

    pub fn position(&self, symbol: &str) -> Option<&AssetPosition> {
        self.positions.get(symbol)
    }

    pub fn equity(&self) -> f64 {
        self.cash + self.positions.values().map(AssetPosition::market_value).sum::<f64>()
    }

    /// 每个新交易日开始时，将此前交易日买入的股票转为可卖数量。
    pub(crate) fn begin_day(&mut self) {
        for position in self.positions.values_mut() {
            position.sellable_quantity += position.bought_today;
            position.bought_today = 0;
        }
    }

    pub(crate) fn update_prices(&mut self, closes: &std::collections::BTreeMap<String, f64>) {
        for (symbol, close) in closes {
            if let Some(position) = self.positions.get_mut(symbol) {
                position.last_price = Some(*close);
            }
        }
    }

    pub(crate) fn apply_buy(
        &mut self,
        symbol: &str,
        quantity: u64,
        price: f64,
        fee: f64,
    ) -> Result<(), String> {
        if quantity == 0 || !price.is_finite() || price <= 0.0 || !fee.is_finite() || fee < 0.0 {
            return Err("买入成交参数无效".into());
        }
        let total_cost = quantity as f64 * price + fee;
        if total_cost > self.cash + f64::EPSILON {
            return Err("组合可用现金不足".into());
        }

        let position = self.positions
            .entry(symbol.to_string())
            .or_insert_with(|| AssetPosition::new(symbol.to_string()));
        let old_cost = position.average_cost * position.quantity as f64;
        position.quantity += quantity;
        position.bought_today += quantity;
        position.average_cost = (old_cost + total_cost) / position.quantity as f64;
        position.last_price = Some(price);
        self.cash -= total_cost;
        Ok(())
    }

    pub(crate) fn apply_sell(
        &mut self,
        symbol: &str,
        quantity: u64,
        price: f64,
        fee: f64,
    ) -> Result<(), String> {
        let position = self.positions.get_mut(symbol)
            .ok_or_else(|| format!("{symbol} 没有可卖持仓"))?;
        if quantity == 0 || quantity > position.sellable_quantity || quantity > position.quantity {
            return Err(format!("{symbol} 可卖数量不足"));
        }
        if !price.is_finite() || price <= 0.0 || !fee.is_finite() || fee < 0.0 {
            return Err("卖出成交参数无效".into());
        }

        let proceeds = quantity as f64 * price - fee;
        position.realized_pnl += proceeds - position.average_cost * quantity as f64;
        position.quantity -= quantity;
        position.sellable_quantity -= quantity;
        position.last_price = Some(price);
        self.cash += proceeds;
        if position.quantity == 0 {
            position.average_cost = 0.0;
            position.last_price = None;
        }
        Ok(())
    }

    pub(crate) fn snapshot(
        &mut self,
        date: &str,
        closes: &std::collections::BTreeMap<String, f64>,
    ) -> MultiAssetSnapshot {
        self.update_prices(closes);
        let positions: std::collections::BTreeMap<String, AssetPositionSnapshot> = self.positions
            .iter()
            .filter(|(_, p)| p.quantity > 0)
            .map(|(symbol, p)| (symbol.clone(), AssetPositionSnapshot {
                symbol: symbol.clone(),
                quantity: p.quantity,
                sellable_quantity: p.sellable_quantity,
                close: p.last_price,
                market_value: p.market_value(),
                average_cost: p.average_cost,
                unrealized_pnl: p.unrealized_pnl(),
                realized_pnl: p.realized_pnl,
            }))
            .collect();
        let market_value = positions.values().map(|p| p.market_value).sum::<f64>();
        let equity = self.cash + market_value;
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
            cash: self.cash,
            positions,
            market_value,
            equity,
            daily_return,
            drawdown,
        }
    }
}
