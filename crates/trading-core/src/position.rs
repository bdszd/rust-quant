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
    pub(crate) fn new(symbol: String) -> Self {
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