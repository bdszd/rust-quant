mod buy_and_hold;
mod sma_cross;

pub use buy_and_hold::BuyAndHoldStrategy;
pub use sma_cross::SmaCrossStrategy;

use crate::data::Bar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Buy,
    Sell,
    Hold,
}

pub trait Strategy {
    fn on_bar(&mut self, bar: &Bar) -> Signal;
}


/// 组合策略：每个交易日收盘后，为一个或多个标的给出目标权重。
/// 例如 000001.SZ=0.3、600000.SH=0.2，剩余资金保留为现金。
#[derive(Debug, Clone, PartialEq)]
pub struct TargetWeight {
    pub symbol: String,
    pub weight: f64,
}

pub trait PortfolioStrategy {
    fn on_day(
        &mut self,
        date: &str,
        bars: &std::collections::BTreeMap<String, Bar>,
        portfolio: &crate::portfolio::MultiAssetSnapshot,
    ) -> Vec<TargetWeight>;
}

/// 静态目标权重策略，适合验证组合回测和基准链路。
pub struct FixedWeightStrategy {
    weights: Vec<TargetWeight>,
}

impl FixedWeightStrategy {
    pub fn new(weights: Vec<TargetWeight>) -> Self {
        let total: f64 = weights.iter().map(|w| w.weight).sum();
        assert!(weights.iter().all(|w| !w.symbol.trim().is_empty()
            && w.weight.is_finite() && (0.0..=1.0).contains(&w.weight)));
        assert!(total <= 1.0 + f64::EPSILON, "目标权重之和不能超过 100%");
        Self { weights }
    }
}

impl PortfolioStrategy for FixedWeightStrategy {
    fn on_day(
        &mut self,
        _date: &str,
        _bars: &std::collections::BTreeMap<String, Bar>,
        _portfolio: &crate::portfolio::MultiAssetSnapshot,
    ) -> Vec<TargetWeight> {
        self.weights.clone()
    }
}
