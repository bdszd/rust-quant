use crate::portfolio::Portfolio;
use crate::strategy::Signal;
use serde::{Deserialize, Serialize};
use trading_core::{Fill, Order, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[serde(alias = "NextOpen")]
    NextOpen,
    #[serde(alias = "NextClose")]
    NextClose,
}

pub type Trade = Fill;

/// V1 模拟执行器：Signal -> Order -> Fill -> Portfolio。
pub struct ExecutionModel {
    commission_rate: f64,
    slippage_rate: f64,
    lot_size: u64,
    stamp_duty_rate: f64,
    min_commission: f64,
    next_order_id: u64,
    trades: Vec<Fill>,
}

impl ExecutionModel {
    pub(crate) fn new(
        commission_rate: f64,
        slippage_rate: f64,
        lot_size: u64,
        stamp_duty_rate: f64,
        min_commission: f64,
    ) -> Self {
        Self {
            commission_rate,
            slippage_rate,
            lot_size,
            stamp_duty_rate,
            min_commission,
            next_order_id: 1,
            trades: Vec::new(),
        }
    }

    pub fn execute(
        &mut self,
        portfolio: &mut Portfolio,
        signal: Signal,
        symbol: &str,
        date: &str,
        market_price: f64,
    ) -> Result<(), String> {
        if signal == Signal::Hold {
            return Ok(());
        }
        if !market_price.is_finite() || market_price <= 0.0 {
            return Err("成交参考价格必须是有限的正数".into());
        }

        let side = match signal {
            Signal::Buy if portfolio.position() == 0 => Side::Buy,
            Signal::Sell if portfolio.position() > 0 => Side::Sell,
            _ => return Ok(()),
        };

        let fill_price = match side {
            Side::Buy => market_price * (1.0 + self.slippage_rate),
            Side::Sell => market_price * (1.0 - self.slippage_rate),
        };
        if !fill_price.is_finite() || fill_price <= 0.0 {
            return Err("滑点调整后的成交价格无效".into());
        }

        let quantity = match side {
            Side::Buy => {
                let cost_per_share = fill_price * (1.0 + self.commission_rate);
                let raw_quantity = (portfolio.cash() / cost_per_share).floor() as u64;
                let mut quantity = raw_quantity / self.lot_size * self.lot_size;

                // 最低佣金可能使按比例佣金估算出的买入数量略超出现金，按整手回退。
                while quantity > 0 {
                    let amount = quantity as f64 * fill_price;
                    let fee = (amount * self.commission_rate).max(self.min_commission);
                    if amount + fee <= portfolio.cash() {
                        break;
                    }
                    quantity -= self.lot_size;
                }
                quantity
            }
            Side::Sell => portfolio.position(),
        };
        if quantity == 0 {
            return Ok(());
        }

        let order_id = self.allocate_order_id();
        let order = Order::market(order_id, symbol, side, quantity);
        order.validate()?;

        let amount = quantity as f64 * fill_price;
        let commission = (amount * self.commission_rate).max(self.min_commission);
        let stamp_duty = if side == Side::Sell {
            amount * self.stamp_duty_rate
        } else {
            0.0
        };
        let fee = commission + stamp_duty;
        let fill = Fill {
            order_id: order.id,
            date: date.to_string(),
            symbol: order.symbol.clone(),
            side: order.side,
            quantity: order.quantity,
            price: fill_price,
            fee,
        };
        fill.validate()?;

        portfolio.apply_fill(&fill)?;
        self.trades.push(fill);
        Ok(())
    }

    fn allocate_order_id(&mut self) -> u64 {
        let id = self.next_order_id;
        self.next_order_id += 1;
        id
    }

    pub fn trades(&self) -> &[Fill] {
        &self.trades
    }
}

/// V2 多资产组合执行器。卖单先于买单处理，避免买单抢占尚未释放的卖出资金。
pub struct MultiAssetExecutionModel {
    commission_rate: f64,
    slippage_rate: f64,
    lot_size: u64,
    stamp_duty_rate: f64,
    min_commission: f64,
    next_order_id: u64,
    trades: Vec<Fill>,
}

impl MultiAssetExecutionModel {
    pub(crate) fn new(
        commission_rate: f64,
        slippage_rate: f64,
        lot_size: u64,
        stamp_duty_rate: f64,
        min_commission: f64,
    ) -> Self {
        Self {
            commission_rate,
            slippage_rate,
            lot_size,
            stamp_duty_rate,
            min_commission,
            next_order_id: 1,
            trades: Vec::new(),
        }
    }

    pub(crate) fn execute_targets(
        &mut self,
        portfolio: &mut crate::portfolio::MultiAssetPortfolio,
        targets: &[crate::strategy::TargetWeight],
        bars: &std::collections::BTreeMap<String, crate::data::Bar>,
        date: &str,
        mode: ExecutionMode,
    ) -> Result<(), String> {
        use std::collections::{BTreeMap, BTreeSet};
        let mut weights = BTreeMap::new();
        let mut total_weight = 0.0;
        for target in targets {
            if target.symbol.trim().is_empty()
                || !target.weight.is_finite()
                || !(0.0..=1.0).contains(&target.weight)
            {
                return Err("目标权重或股票代码无效".into());
            }
            if weights
                .insert(target.symbol.clone(), target.weight)
                .is_some()
            {
                return Err(format!("目标权重中存在重复股票：{}", target.symbol));
            }
            total_weight += target.weight;
        }
        if total_weight > 1.0 + f64::EPSILON {
            return Err("目标权重之和不能超过 100%".into());
        }

        let price_of = |bar: &crate::data::Bar| match mode {
            ExecutionMode::NextOpen => bar.open,
            ExecutionMode::NextClose => bar.close,
        };
        let mut execution_prices = BTreeMap::new();
        for (symbol, bar) in bars {
            let price = price_of(bar);
            // 无成交量的交易日不撮合订单；仍可在收盘时使用行情价格估值。
            if !price.is_finite() || price <= 0.0 || bar.volume <= 0.0 {
                continue;
            }
            execution_prices.insert(symbol.clone(), price);
        }
        portfolio.update_prices(&execution_prices);
        let equity = portfolio.equity();
        if !equity.is_finite() || equity <= 0.0 {
            return Err("组合权益无效，无法计算目标权重".into());
        }
        let preserved_weight: f64 = portfolio
            .positions()
            .iter()
            .filter(|(symbol, _)| !weights.contains_key(*symbol))
            .map(|(_, position)| position.market_value() / equity)
            .sum();
        if total_weight + preserved_weight > 1.0 + f64::EPSILON {
            return Err("显式目标权重与未指定持仓的保留权重之和超过 100%".into());
        }

        // 未指定的现有持仓保持当前权重；显式设置为 0 才会退出持仓。
        let symbols: BTreeSet<String> = portfolio
            .positions()
            .keys()
            .cloned()
            .chain(weights.keys().cloned())
            .collect();

        // 第一阶段：卖出超出目标权重的部分。
        for symbol in &symbols {
            let Some(bar) = bars.get(symbol) else {
                continue;
            };
            let Some(market_price) = execution_prices.get(symbol).copied() else {
                continue;
            };
            let target_weight = weights.get(symbol).copied().unwrap_or_else(|| {
                portfolio
                    .position(symbol)
                    .map(|p| p.market_value() / equity)
                    .unwrap_or(0.0)
            });
            let target_value = equity * target_weight;
            let target_quantity =
                (target_value / market_price).floor() as u64 / self.lot_size * self.lot_size;
            let Some(position) = portfolio.position(symbol) else {
                continue;
            };
            let current_quantity = position.quantity;
            let sellable = position.sellable_quantity;
            let quantity = if target_weight == 0.0 {
                sellable
            } else {
                current_quantity
                    .saturating_sub(target_quantity)
                    .min(sellable)
                    / self.lot_size
                    * self.lot_size
            };
            if quantity == 0 {
                continue;
            }
            let fill_price = market_price * (1.0 - self.slippage_rate);
            let amount = quantity as f64 * fill_price;
            let commission = (amount * self.commission_rate).max(self.min_commission);
            let fee = commission + amount * self.stamp_duty_rate;
            let order_id = self.allocate_order_id();
            let fill = Fill {
                order_id,
                date: date.to_string(),
                symbol: symbol.clone(),
                side: Side::Sell,
                quantity,
                price: fill_price,
                fee,
            };
            fill.validate()?;
            portfolio.apply_fill(&fill)?;
            self.trades.push(fill);
            let _ = bar;
        }

        // 第二阶段：用卖出后释放的现金执行买入；逐标的按字典序执行，结果可复现。
        for symbol in &symbols {
            let Some(bar) = bars.get(symbol) else {
                continue;
            };
            let Some(market_price) = execution_prices.get(symbol).copied() else {
                continue;
            };
            let target_weight = weights.get(symbol).copied().unwrap_or_else(|| {
                portfolio
                    .position(symbol)
                    .map(|p| p.market_value() / equity)
                    .unwrap_or(0.0)
            });
            if target_weight <= 0.0 {
                continue;
            }
            let target_value = equity * target_weight;
            let current_quantity = portfolio.position(symbol).map(|p| p.quantity).unwrap_or(0);
            let target_quantity =
                (target_value / market_price).floor() as u64 / self.lot_size * self.lot_size;
            let desired_quantity = target_quantity.saturating_sub(current_quantity);
            if desired_quantity == 0 {
                continue;
            }

            let fill_price = market_price * (1.0 + self.slippage_rate);
            let cost_per_share = fill_price * (1.0 + self.commission_rate);
            let affordable =
                (portfolio.cash() / cost_per_share).floor() as u64 / self.lot_size * self.lot_size;
            let mut quantity = desired_quantity.min(affordable);
            let mut amount = quantity as f64 * fill_price;
            let mut fee = if quantity == 0 {
                0.0
            } else {
                (amount * self.commission_rate).max(self.min_commission)
            };
            while quantity > 0 && amount + fee > portfolio.cash() + f64::EPSILON {
                quantity = quantity.saturating_sub(self.lot_size);
                amount = quantity as f64 * fill_price;
                fee = if quantity == 0 {
                    0.0
                } else {
                    (amount * self.commission_rate).max(self.min_commission)
                };
            }
            if quantity == 0 {
                continue;
            }
            let order_id = self.allocate_order_id();
            let fill = Fill {
                order_id,
                date: date.to_string(),
                symbol: symbol.clone(),
                side: Side::Buy,
                quantity,
                price: fill_price,
                fee,
            };
            fill.validate()?;
            portfolio.apply_fill(&fill)?;
            self.trades.push(fill);
            let _ = bar;
        }
        Ok(())
    }

    fn allocate_order_id(&mut self) -> u64 {
        let id = self.next_order_id;
        self.next_order_id += 1;
        id
    }

    pub fn trades(&self) -> &[Fill] {
        &self.trades
    }
}
