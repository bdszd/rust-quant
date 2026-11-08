use std::collections::BTreeMap;

use crate::error::AccountError;
use crate::fill::Fill;
use crate::order::Side;
use crate::position::Position;

/// 交易账户：拥有现金、持仓和成交后的账户状态变更规则。
/// 不负责读取行情、计算组合收益率或生成回测快照。
#[derive(Debug, Clone)]
pub struct Account {
    cash: f64,
    positions: BTreeMap<String, Position>,
}

impl Account {
    pub fn new(initial_cash: f64) -> Result<Self, AccountError> {
        if !initial_cash.is_finite() || initial_cash <= 0.0 {
            return Err(AccountError::InvalidInitialCash);
        }

        Ok(Self {
            cash: initial_cash,
            positions: BTreeMap::new(),
        })
    }

    pub fn cash(&self) -> f64 {
        self.cash
    }

    pub fn positions(&self) -> &BTreeMap<String, Position> {
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

    /// 更新已有持仓的最新参考价格。
    /// 该价格不参与账户现金或持仓数量变更。
    pub fn update_prices(&mut self, prices: &BTreeMap<String, f64>) {
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
                    .ok_or_else(|| {
                        AccountError::PositionNotFound(fill.symbol.clone())
                    })?;

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
mod tests {
    use super::*;

    fn fill(
        side: Side,
        date: &str,
        quantity: u64,
        price: f64,
        fee: f64,
    ) -> Fill {
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

        let same_day_sell =
            fill(Side::Sell, "2025-01-02", 100, 11.0, 0.0);

        assert!(matches!(
            account.apply_fill(&same_day_sell),
            Err(AccountError::InsufficientSellableQuantity { .. })
        ));

        account.begin_day();

        let next_day_sell =
            fill(Side::Sell, "2025-01-03", 100, 11.0, 0.0);

        account.apply_fill(&next_day_sell).unwrap();

        assert_eq!(account.position("AAA").unwrap().quantity, 0);
        assert_eq!(account.cash(), 10_095.0);
    }

    #[test]
    fn failed_buy_does_not_change_account_state() {
        let mut account = Account::new(100.0).unwrap();

        let buy = fill(Side::Buy, "2025-01-02", 100, 10.0, 1.0);

        assert_eq!(
            account.apply_fill(&buy),
            Err(AccountError::InsufficientCash)
        );

        assert_eq!(account.cash(), 100.0);
        assert!(account.position("AAA").is_none());
    }
}