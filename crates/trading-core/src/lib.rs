use serde::{Deserialize, Serialize};

/// 订单方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

/// 一笔交易请求。V1 撮合市价单，limit_price 保留给后续限价单。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: u64,
    pub symbol: String,
    pub side: Side,
    pub quantity: u64,
    pub limit_price: Option<f64>,
}

impl Order {
    pub fn market(id: u64, symbol: impl Into<String>, side: Side, quantity: u64) -> Self {
        Self {
            id,
            symbol: symbol.into(),
            side,
            quantity,
            limit_price: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.symbol.trim().is_empty() {
            return Err("symbol 不能为空".into());
        }
        if self.quantity == 0 {
            return Err("订单数量必须大于 0".into());
        }
        if let Some(price) = self.limit_price
            && (!price.is_finite() || price <= 0.0)
        {
            return Err("限价必须是有限的正数".into());
        }
        Ok(())
    }
}

/// 一笔已经完成的成交。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: u64,
    pub date: String,
    pub symbol: String,
    pub side: Side,
    pub quantity: u64,
    pub price: f64,
    pub fee: f64,
}

impl Fill {
    pub fn validate(&self) -> Result<(), String> {
        if self.symbol.trim().is_empty() {
            return Err("symbol 不能为空".into());
        }
        if self.date.trim().is_empty() {
            return Err("成交日期不能为空".into());
        }
        if self.quantity == 0 {
            return Err("成交数量必须大于 0".into());
        }
        if !self.price.is_finite() || self.price <= 0.0 {
            return Err("成交价格必须是有限的正数".into());
        }
        if !self.fee.is_finite() || self.fee < 0.0 {
            return Err("手续费必须是有限的非负数".into());
        }
        Ok(())
    }
}


/// 账户持仓。只保存交易状态；行情估值由上层系统负责。
#[derive(Debug, Clone)]
pub struct Position {
    pub symbol: String,
    pub quantity: u64,
    pub sellable_quantity: u64,
    pub average_cost: f64,
    pub last_price: Option<f64>,
    pub realized_pnl: f64,
    pub bought_today: u64,
}

impl Position {
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
        (self.last_price.unwrap_or(self.average_cost) - self.average_cost)
            * self.quantity as f64
    }
}

/// 账户操作失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountError {
    InvalidInitialCash,
    InvalidFill(String),
    InvalidAmount,
    InsufficientCash,
    PositionNotFound(String),
    InsufficientSellableQuantity {
        symbol: String,
        requested: u64,
        available: u64,
    },
    QuantityOverflow,
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInitialCash => write!(f, "初始资金必须是有限的正数"),
            Self::InvalidFill(message) => write!(f, "成交记录无效：{message}"),
            Self::InvalidAmount => write!(f, "成交金额超出有效数值范围"),
            Self::InsufficientCash => write!(f, "账户可用现金不足"),
            Self::PositionNotFound(symbol) => write!(f, "{symbol} 没有持仓"),
            Self::InsufficientSellableQuantity { symbol, requested, available } => {
                write!(f, "{symbol} 可卖数量不足：请求 {requested}，可卖 {available}")
            }
            Self::QuantityOverflow => write!(f, "持仓数量溢出"),
        }
    }
}

impl std::error::Error for AccountError {}

/// 交易账户：拥有现金、持仓和成交后的账户状态变更规则。
/// 不负责读取行情、计算组合收益率或生成回测快照。
#[derive(Debug, Clone)]
pub struct Account {
    cash: f64,
    positions: std::collections::BTreeMap<String, Position>,
}

impl Account {
    pub fn new(initial_cash: f64) -> Result<Self, AccountError> {
        if !initial_cash.is_finite() || initial_cash <= 0.0 {
            return Err(AccountError::InvalidInitialCash);
        }
        Ok(Self {
            cash: initial_cash,
            positions: std::collections::BTreeMap::new(),
        })
    }

    pub fn cash(&self) -> f64 {
        self.cash
    }

    pub fn positions(&self) -> &std::collections::BTreeMap<String, Position> {
        &self.positions
    }

    pub fn position(&self, symbol: &str) -> Option<&Position> {
        self.positions.get(symbol)
    }

    /// 新交易日开始时，昨日买入的数量转为可卖数量。
    pub fn begin_day(&mut self) {
        for position in self.positions.values_mut() {
            position.sellable_quantity += position.bought_today;
            position.bought_today = 0;
        }
    }

    /// 更新已有持仓的最新参考价格。该价格不参与账户现金或持仓数量变更。
    pub fn update_prices(
        &mut self,
        prices: &std::collections::BTreeMap<String, f64>,
    ) {
        for (symbol, price) in prices {
            if let Some(position) = self.positions.get_mut(symbol) {
                position.last_price = Some(*price);
            }
        }
    }

    /// 以成交记录作为唯一账户变更入口。
    pub fn apply_fill(&mut self, fill: &Fill) -> Result<(), AccountError> {
        fill.validate().map_err(AccountError::InvalidFill)?;
        let amount = fill.quantity as f64 * fill.price;
        if !amount.is_finite() {
            return Err(AccountError::InvalidAmount);
        }

        match fill.side {
            Side::Buy => {
                let total_cost = amount + fill.fee;
                if !total_cost.is_finite() {
                    return Err(AccountError::InvalidAmount);
                }
                if total_cost > self.cash + f64::EPSILON {
                    return Err(AccountError::InsufficientCash);
                }

                let position = self
                    .positions
                    .entry(fill.symbol.clone())
                    .or_insert_with(|| Position::new(fill.symbol.clone()));
                let new_quantity = position
                    .quantity
                    .checked_add(fill.quantity)
                    .ok_or(AccountError::QuantityOverflow)?;
                let new_bought_today = position
                    .bought_today
                    .checked_add(fill.quantity)
                    .ok_or(AccountError::QuantityOverflow)?;
                let old_cost = position.average_cost * position.quantity as f64;
                position.quantity = new_quantity;
                position.bought_today = new_bought_today;
                position.average_cost =
                    (old_cost + total_cost) / position.quantity as f64;
                position.last_price = Some(fill.price);
                self.cash -= total_cost;
            }
            Side::Sell => {
                let position = self
                    .positions
                    .get_mut(&fill.symbol)
                    .ok_or_else(|| AccountError::PositionNotFound(fill.symbol.clone()))?;
                if fill.quantity == 0 || fill.quantity > position.sellable_quantity {
                    return Err(AccountError::InsufficientSellableQuantity {
                        symbol: fill.symbol.clone(),
                        requested: fill.quantity,
                        available: position.sellable_quantity,
                    });
                }

                let proceeds = amount - fill.fee;
                if !proceeds.is_finite() || proceeds < 0.0 {
                    return Err(AccountError::InvalidAmount);
                }
                position.realized_pnl +=
                    proceeds - position.average_cost * fill.quantity as f64;
                position.quantity -= fill.quantity;
                position.sellable_quantity -= fill.quantity;
                position.last_price = Some(fill.price);
                self.cash += proceeds;
                if position.quantity == 0 {
                    position.average_cost = 0.0;
                    position.last_price = None;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod account_tests {
    use super::*;

    fn fill(side: Side, date: &str, quantity: u64, price: f64, fee: f64) -> Fill {
        Fill {
            order_id: 1,
            date: date.into(),
            symbol: "AAA".into(),
            side,
            quantity,
            price,
            fee,
        }
    }

    #[test]
    fn account_applies_fill_and_enforces_t_plus_one() {
        let mut account = Account::new(10_000.0).unwrap();
        let buy = fill(Side::Buy, "2025-01-02", 100, 10.0, 5.0);
        account.apply_fill(&buy).unwrap();

        let position = account.position("AAA").unwrap();
        assert_eq!(position.quantity, 100);
        assert_eq!(position.sellable_quantity, 0);
        assert_eq!(position.bought_today, 100);
        assert_eq!(account.cash(), 8_995.0);

        let same_day_sell = fill(Side::Sell, "2025-01-02", 100, 11.0, 0.0);
        assert!(matches!(
            account.apply_fill(&same_day_sell),
            Err(AccountError::InsufficientSellableQuantity { .. })
        ));

        account.begin_day();
        let next_day_sell = fill(Side::Sell, "2025-01-03", 100, 11.0, 0.0);
        account.apply_fill(&next_day_sell).unwrap();
        assert_eq!(account.position("AAA").unwrap().quantity, 0);
        assert_eq!(account.cash(), 10_095.0);
    }

    #[test]
    fn failed_buy_does_not_change_account_state() {
        let mut account = Account::new(100.0).unwrap();
        let buy = fill(Side::Buy, "2025-01-02", 100, 10.0, 1.0);
        assert_eq!(account.apply_fill(&buy), Err(AccountError::InsufficientCash));
        assert_eq!(account.cash(), 100.0);
        assert!(account.position("AAA").is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_validates_market_order() {
        let order = Order::market(1, "000001.SZ", Side::Buy, 100);
        assert!(order.validate().is_ok());
        assert_eq!(order.limit_price, None);
    }

    #[test]
    fn rejects_zero_quantity() {
        let order = Order::market(2, "000001.SZ", Side::Buy, 0);
        assert!(order.validate().is_err());
    }

    #[test]
    fn rejects_invalid_fill_price() {
        let fill = Fill {
            order_id: 1,
            date: "2025-01-01".into(),
            symbol: "000001.SZ".into(),
            side: Side::Buy,
            quantity: 10,
            price: f64::NAN,
            fee: 1.0,
        };
        assert!(fill.validate().is_err());
    }
}
