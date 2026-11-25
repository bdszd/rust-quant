pub mod core;

pub mod momentum;
pub mod price;
pub mod statistics;
pub mod trend;
pub mod volatility;
pub mod volume;

// 暂时保留旧版 SMA，保证现有策略兼容。
pub mod sma;

pub use core::{Indicator, IndicatorError, IndicatorSeries};

pub use sma::sma;
