use crate::data::Bar;

use crate::indicators::core::{Indicator, IndicatorError, IndicatorSeries};

/// 指数移动平均线（Exponential Moving Average）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ema {
    window: usize,
}

impl Ema {
    pub fn new(window: usize) -> Self {
        Self { window }
    }

    pub fn window(&self) -> usize {
        self.window
    }
}

impl Indicator for Ema {
    type Output = IndicatorSeries;

    fn calculate(&self, bars: &[Bar]) -> Result<Self::Output, IndicatorError> {
        let prices: Vec<f64> = bars.iter().map(|bar| bar.close).collect();

        let values = calculate_ema(&prices, self.window)?;

        Ok(IndicatorSeries::new(values))
    }
}

/// 根据价格序列计算 EMA。
///
/// 返回结果与 prices 等长。
/// 数据不足的位置使用 None 表示。
///
/// pub(super) 表示这个函数对 trend 模块及其内部模块可见，
/// 其他无关模块不能直接调用。
pub(super) fn calculate_ema(
    prices: &[f64],
    window: usize,
) -> Result<Vec<Option<f64>>, IndicatorError> {
    if window == 0 {
        return Err(IndicatorError::InvalidPeriod { period: window });
    }

    let mut values = vec![None; prices.len()];

    for (index, price) in prices.iter().enumerate() {
        if !price.is_finite() {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "收盘价不是有效的有限数值",
            });
        }
    }

    if prices.len() < window {
        return Ok(values);
    }

    let alpha = 2.0 / (window as f64 + 1.0);

    let initial_sum: f64 = prices[..window].iter().sum();

    if !initial_sum.is_finite() {
        return Err(IndicatorError::NonFiniteOutput { index: window - 1 });
    }

    let mut previous_ema = initial_sum / window as f64;

    if !previous_ema.is_finite() {
        return Err(IndicatorError::NonFiniteOutput { index: window - 1 });
    }

    values[window - 1] = Some(previous_ema);

    for index in window..prices.len() {
        let current_price = prices[index];

        let current_ema = alpha * current_price + (1.0 - alpha) * previous_ema;

        if !current_ema.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        values[index] = Some(current_ema);
        previous_ema = current_ema;
    }

    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::calculate_ema;

    #[test]
    fn calculates_ema_with_sma_initialization() {
        let prices = [10.0, 12.0, 14.0, 16.0];

        let values = calculate_ema(&prices, 3).unwrap();

        assert_eq!(values[0], None);
        assert_eq!(values[1], None);
        assert_eq!(values[2], Some(12.0));
        assert_eq!(values[3], Some(14.0));
    }

    #[test]
    fn returns_none_when_data_is_insufficient() {
        let prices = [10.0, 12.0];

        let values = calculate_ema(&prices, 3).unwrap();

        assert_eq!(values, vec![None, None]);
    }

    #[test]
    fn returns_error_for_zero_window() {
        let result = calculate_ema(&[10.0, 12.0], 0);

        assert!(result.is_err());
    }

    #[test]
    fn returns_error_for_non_finite_price() {
        let prices = [10.0, f64::NAN, 14.0];

        let result = calculate_ema(&prices, 2);

        assert!(result.is_err());
    }
}
