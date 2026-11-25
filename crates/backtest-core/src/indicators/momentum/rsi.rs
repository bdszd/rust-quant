use crate::data::Bar;

use crate::indicators::core::{Indicator, IndicatorError, IndicatorSeries};

/// 相对强弱指标（Relative Strength Index）。
///
/// 使用 Wilder 平滑算法计算 RSI。
///
/// RSI 的取值范围为 0～100。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rsi {
    period: usize,
}

impl Rsi {
    /// 创建 RSI 指标。
    ///
    /// 常见周期为 14。
    pub fn new(period: usize) -> Self {
        Self { period }
    }

    pub fn period(&self) -> usize {
        self.period
    }
}

impl Indicator for Rsi {
    type Output = IndicatorSeries;

    fn calculate(&self, bars: &[Bar]) -> Result<Self::Output, IndicatorError> {
        let prices: Vec<f64> = bars.iter().map(|bar| bar.close).collect();

        let values = calculate_rsi(&prices, self.period)?;

        Ok(IndicatorSeries::new(values))
    }
}

/// 根据价格序列计算 RSI。
///
/// 返回序列与输入价格等长。
///
/// 第一个有效 RSI 出现在 period 位置，
/// 因为计算 period 次价格变化需要 period + 1 个价格。
fn calculate_rsi(prices: &[f64], period: usize) -> Result<Vec<Option<f64>>, IndicatorError> {
    if period == 0 {
        return Err(IndicatorError::InvalidPeriod { period });
    }

    let mut values = vec![None; prices.len()];

    // 检查输入数据。
    for (index, price) in prices.iter().enumerate() {
        if !price.is_finite() {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "收盘价不是有效的有限数值",
            });
        }
    }

    // RSI 至少需要 period + 1 个价格。
    if prices.len() <= period {
        return Ok(values);
    }

    let mut total_gain = 0.0;
    let mut total_loss = 0.0;

    // 1. 计算初始平均上涨和平均下跌幅度。
    for index in 1..=period {
        let change = prices[index] - prices[index - 1];

        if !change.is_finite() {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "价格变化超出有限数值范围",
            });
        }

        if change > 0.0 {
            total_gain += change;
        } else {
            total_loss += -change;
        }

        if !total_gain.is_finite() || !total_loss.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }
    }

    let divisor = period as f64;

    let mut average_gain = total_gain / divisor;
    let mut average_loss = total_loss / divisor;

    // 2. 计算第一个 RSI。
    values[period] = Some(calculate_rsi_value(average_gain, average_loss));

    // 3. 使用 Wilder 平滑算法计算后续 RSI。
    for index in (period + 1)..prices.len() {
        let change = prices[index] - prices[index - 1];

        if !change.is_finite() {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "价格变化超出有限数值范围",
            });
        }

        let gain = change.max(0.0);
        let loss = (-change).max(0.0);

        average_gain = (average_gain * (divisor - 1.0) + gain) / divisor;

        average_loss = (average_loss * (divisor - 1.0) + loss) / divisor;

        if !average_gain.is_finite() || !average_loss.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        values[index] = Some(calculate_rsi_value(average_gain, average_loss));
    }

    Ok(values)
}

/// 根据平均上涨和平均下跌幅度计算 RSI。
fn calculate_rsi_value(average_gain: f64, average_loss: f64) -> f64 {
    // 没有上涨，也没有下跌：价格保持不变。
    if average_gain == 0.0 && average_loss == 0.0 {
        return 50.0;
    }

    // 只有上涨，没有下跌。
    if average_loss == 0.0 {
        return 100.0;
    }

    // 只有下跌，没有上涨。
    if average_gain == 0.0 {
        return 0.0;
    }

    let rs = average_gain / average_loss;

    100.0 - 100.0 / (1.0 + rs)
}

#[cfg(test)]
mod tests {
    use super::calculate_rsi;

    #[test]
    fn calculates_rsi_with_wilder_smoothing() {
        let prices = [10.0, 12.0, 11.0, 14.0, 13.0, 15.0];

        let values = calculate_rsi(&prices, 3).unwrap();

        assert_eq!(values[0], None);
        assert_eq!(values[1], None);
        assert_eq!(values[2], None);

        // 第一次计算：
        // 平均上涨 = (2 + 0 + 3) / 3 = 1.6667
        // 平均下跌 = (0 + 1 + 0) / 3 = 0.3333
        // RSI = 83.3333
        assert!((values[3].unwrap() - 83.3333333333).abs() < 1e-8);

        assert!(values[4].is_some());
        assert!(values[5].is_some());
    }

    #[test]
    fn returns_none_when_data_is_insufficient() {
        let prices = [10.0, 12.0, 11.0];

        let values = calculate_rsi(&prices, 3).unwrap();

        assert_eq!(values, vec![None, None, None]);
    }

    #[test]
    fn returns_100_when_prices_only_rise() {
        let prices = [10.0, 11.0, 12.0, 13.0];

        let values = calculate_rsi(&prices, 3).unwrap();

        assert_eq!(values[3], Some(100.0));
    }

    #[test]
    fn returns_zero_when_prices_only_fall() {
        let prices = [13.0, 12.0, 11.0, 10.0];

        let values = calculate_rsi(&prices, 3).unwrap();

        assert_eq!(values[3], Some(0.0));
    }

    #[test]
    fn returns_50_when_prices_are_flat() {
        let prices = [10.0, 10.0, 10.0, 10.0];

        let values = calculate_rsi(&prices, 3).unwrap();

        assert_eq!(values[3], Some(50.0));
    }

    #[test]
    fn returns_error_for_zero_period() {
        let result = calculate_rsi(&[10.0, 11.0], 0);

        assert!(result.is_err());
    }

    #[test]
    fn returns_error_for_non_finite_price() {
        let prices = [10.0, 11.0, f64::NAN, 13.0];

        let result = calculate_rsi(&prices, 2);

        assert!(result.is_err());
    }
}
