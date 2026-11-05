use crate::portfolio::Portfolio;
use crate::strategy::Signal;
use serde::{Deserialize, Serialize};
use trading_core::{Fill, Side};

/// 回测中的模拟成交时机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    NextOpen,
    NextClose,
}

/// 保留旧名称，避免现有调用方立即全部改名。
pub type Trade = Fill;

/// 将策略信号转化为模拟成交。
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
        date: &str,
        market_price: f64,
    ) {
        if signal == Signal::Hold {
            return;
        }

        assert!(
            market_price.is_finite() && market_price > 0.0,
            "成交参考价格必须是有限的正数"
        );

        match signal {
            Signal::Buy if portfolio.position() == 0 => {
                let fill_price = market_price * (1.0 + self.slippage_rate);

                let cost_per_share = fill_price * (1.0 + self.commission_rate);

                let quantity = (portfolio.cash() / cost_per_share).floor() as u64;

                if quantity == 0 {
                    return;
                }

                let amount = quantity as f64 * fill_price;
                let fee = amount * self.commission_rate;

                portfolio.apply_buy(quantity, fill_price, fee);

                let order_id = self.allocate_order_id();

                self.trades.push(Fill {
                    order_id,
                    date: date.to_string(),
                    symbol: "AAPL".to_string(),
                    side: Side::Buy,
                    quantity,
                    price: fill_price,
                    fee,
                });
            }

            Signal::Sell if portfolio.position() > 0 => {
                let fill_price = market_price * (1.0 - self.slippage_rate);

                let quantity = portfolio.position();
                let amount = quantity as f64 * fill_price;
                let fee = amount * self.commission_rate;

                portfolio.apply_sell(quantity, fill_price, fee);

                let order_id = self.allocate_order_id();

                self.trades.push(Fill {
                    order_id,
                    date: date.to_string(),
                    symbol: "AAPL".to_string(),
                    side: Side::Sell,
                    quantity,
                    price: fill_price,
                    fee,
                });
            }

            _ => {}
        }
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
