use super::{Signal, Strategy};
use crate::data::Bar;

/// 买入并持有策略：首次收到行情时发出买入信号，之后保持持仓。
pub struct BuyAndHoldStrategy {
    has_signaled_buy: bool,
}

impl BuyAndHoldStrategy {
    pub fn new() -> Self {
        Self {
            has_signaled_buy: false,
        }
    }
}

impl Default for BuyAndHoldStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl Strategy for BuyAndHoldStrategy {
    fn on_bar(&mut self, _bar: &Bar) -> Signal {
        if !self.has_signaled_buy {
            self.has_signaled_buy = true;
            Signal::Buy
        } else {
            Signal::Hold
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Bar;

    fn bar() -> Bar {
        Bar {
            date: "2025-01-01".to_string(),
            open: 10.0,
            high: 11.0,
            low: 9.0,
            close: 10.0,
            volume: 1000,
        }
    }

    #[test]
    fn buys_once_then_holds() {
        let mut strategy = BuyAndHoldStrategy::new();

        assert_eq!(strategy.on_bar(&bar()), Signal::Buy);
        assert_eq!(strategy.on_bar(&bar()), Signal::Hold);
        assert_eq!(strategy.on_bar(&bar()), Signal::Hold);
    }
}
