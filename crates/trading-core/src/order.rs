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
    pub fn market(
        id: u64,
        symbol: impl Into<String>,
        side: Side,
        quantity: u64,
    ) -> Self {
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
}