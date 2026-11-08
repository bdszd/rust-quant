use serde::{Deserialize, Serialize};

use crate::order::Side;

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

#[cfg(test)]
mod tests {
    use super::*;

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