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
    next_order_id: u64,
    trades: Vec<Fill>,
}

impl ExecutionModel {
    pub(crate) fn new(commission_rate: f64, slippage_rate: f64) -> Self {
        Self {
            commission_rate,
            slippage_rate,
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
                (portfolio.cash() / cost_per_share).floor() as u64
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
        let fee = amount * self.commission_rate;
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

        match fill.side {
            Side::Buy => portfolio.apply_buy(fill.quantity, fill.price, fill.fee),
            Side::Sell => portfolio.apply_sell(fill.quantity, fill.price, fill.fee),
        }
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
