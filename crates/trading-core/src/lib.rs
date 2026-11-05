/// 订单方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

/// 一笔交易请求。
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub id: u64,
    pub symbol: String,
    pub side: Side,
    pub quantity: u64,
    pub limit_price: Option<f64>,
}

/// 一笔已经完成的成交。
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    pub order_id: u64,
    pub date: String,
    pub symbol: String,
    pub side: Side,
    pub quantity: u64,
    pub price: f64,
    pub fee: f64,
}

impl Order {
    pub fn validate(&self) -> Result<(), String> {
        if self.symbol.trim().is_empty() {
            return Err("symbol 不能为空".to_string());
        }

        if self.quantity == 0 {
            return Err("订单数量必须大于 0".to_string());
        }

        if let Some(price) = self.limit_price
            && (!price.is_finite() || price <= 0.0)
        {
            return Err("限价必须是有限的正数".to_string());
        }

        Ok(())
    }
}

impl Fill {
    pub fn validate(&self) -> Result<(), String> {
        if self.symbol.trim().is_empty() {
            return Err("symbol 不能为空".to_string());
        }

        if self.quantity == 0 {
            return Err("成交数量必须大于 0".to_string());
        }

        if !self.price.is_finite() || self.price <= 0.0 {
            return Err("成交价格必须是有限的正数".to_string());
        }

        if !self.fee.is_finite() || self.fee < 0.0 {
            return Err("手续费必须是有限的非负数".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_valid_limit_order() {
        let order = Order {
            id: 1,
            symbol: "AAPL".to_string(),
            side: Side::Buy,
            quantity: 100,
            limit_price: Some(200.0),
        };

        assert!(order.validate().is_ok());
    }

    #[test]
    fn rejects_zero_quantity() {
        let order = Order {
            id: 2,
            symbol: "AAPL".to_string(),
            side: Side::Buy,
            quantity: 0,
            limit_price: None,
        };

        assert!(order.validate().is_err());
    }

    #[test]
    fn rejects_invalid_fill_price() {
        let fill = Fill {
            order_id: 1,
            date: "2025-01-01".to_string(),
            symbol: "AAPL".to_string(),
            side: Side::Buy,
            quantity: 10,
            price: f64::NAN,
            fee: 1.0,
        };

        assert!(fill.validate().is_err());
    }
}
