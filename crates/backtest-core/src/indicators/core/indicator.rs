use crate::data::Bar;

use super::error::IndicatorError;

/// 技术指标统一计算接口。
///
/// 不同指标可以拥有不同的输出类型。
pub trait Indicator {
    type Output;

    /// 根据历史行情批量计算指标。
    fn calculate(&self, bars: &[Bar]) -> Result<Self::Output, IndicatorError>;
}
