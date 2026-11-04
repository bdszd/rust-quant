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
