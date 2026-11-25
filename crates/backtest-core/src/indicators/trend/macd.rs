use crate::data::Bar;

use crate::indicators::core::{Indicator, IndicatorError, IndicatorSeries};

use super::ema::calculate_ema;

/// MACD 的完整计算结果。
///
/// DIF：快线 EMA - 慢线 EMA。
/// DEA：DIF 的指数移动平均。
/// Histogram：DIF - DEA。
#[derive(Debug, Clone, PartialEq)]
pub struct MacdOutput {
    pub dif: IndicatorSeries,
    pub dea: IndicatorSeries,
    pub histogram: IndicatorSeries,
}

/// MACD 指标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Macd {
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
}

impl Macd {
    /// 创建 MACD 指标。
    ///
    /// 常见参数为：
    /// fast_period = 12
    /// slow_period = 26
    /// signal_period = 9
    pub fn new(fast_period: usize, slow_period: usize, signal_period: usize) -> Self {
        Self {
            fast_period,
            slow_period,
            signal_period,
        }
    }

    pub fn fast_period(&self) -> usize {
        self.fast_period
    }

    pub fn slow_period(&self) -> usize {
        self.slow_period
    }

    pub fn signal_period(&self) -> usize {
        self.signal_period
    }
}

impl Indicator for Macd {
    type Output = MacdOutput;

    fn calculate(&self, bars: &[Bar]) -> Result<Self::Output, IndicatorError> {
        let prices: Vec<f64> = bars.iter().map(|bar| bar.close).collect();

        calculate_macd(
            &prices,
            self.fast_period,
            self.slow_period,
            self.signal_period,
        )
    }
}

/// 根据价格序列计算 MACD。
fn calculate_macd(
    prices: &[f64],
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> Result<MacdOutput, IndicatorError> {
    // 1. 检查参数。
    for period in [fast_period, slow_period, signal_period] {
        if period == 0 {
            return Err(IndicatorError::InvalidPeriod { period });
        }
    }

    if fast_period >= slow_period {
        return Err(IndicatorError::InvalidParameter {
            name: "fast_period must be less than slow_period",
            value: fast_period as f64,
        });
    }

    // 2. 分别计算快线和慢线 EMA。
    let fast_ema = calculate_ema(prices, fast_period)?;
    let slow_ema = calculate_ema(prices, slow_period)?;

    let len = prices.len();

    // 3. 计算 DIF。
    let mut dif_values = vec![None; len];

    for index in 0..len {
        if let (Some(fast), Some(slow)) = (fast_ema[index], slow_ema[index]) {
            let dif = fast - slow;

            if !dif.is_finite() {
                return Err(IndicatorError::NonFiniteOutput { index });
            }

            dif_values[index] = Some(dif);
        }
    }

    // 慢线第一次有效的位置。
    let first_dif_index = slow_period - 1;

    // 4. 提取有效 DIF，供 DEA 计算使用。
    //
    // DIF 在 first_dif_index 之前都是 None。
    // 提取之后，DEA 使用的是一段连续的有效 DIF 序列。
    let valid_dif: Vec<f64> = dif_values
        .iter()
        .skip(first_dif_index)
        .filter_map(|value| *value)
        .collect();

    // 5. 对 DIF 序列计算 EMA，得到 DEA。
    let signal_ema = calculate_ema(&valid_dif, signal_period)?;

    let mut dea_values = vec![None; len];

    // 将 DEA 结果重新放回原始 K 线对应的位置。
    for (offset, value) in signal_ema.iter().enumerate() {
        let index = first_dif_index + offset;

        if index < len {
            dea_values[index] = *value;
        }
    }

    // 6. 计算 MACD 柱体。
    let mut histogram_values = vec![None; len];

    for index in 0..len {
        if let (Some(dif), Some(dea)) = (dif_values[index], dea_values[index]) {
            let histogram = dif - dea;

            if !histogram.is_finite() {
                return Err(IndicatorError::NonFiniteOutput { index });
            }

            histogram_values[index] = Some(histogram);
        }
    }

    Ok(MacdOutput {
        dif: IndicatorSeries::new(dif_values),
        dea: IndicatorSeries::new(dea_values),
        histogram: IndicatorSeries::new(histogram_values),
    })
}

#[cfg(test)]
mod tests {
    use super::calculate_macd;

    #[test]
    fn calculates_macd_series() {
        let prices = [10.0, 11.0, 12.0, 13.0, 12.0, 14.0, 15.0, 16.0, 15.0, 17.0];

        let result = calculate_macd(&prices, 2, 3, 2).unwrap();

        // 三条输出序列都必须和输入长度一致。
        assert_eq!(result.dif.len(), prices.len());
        assert_eq!(result.dea.len(), prices.len());
        assert_eq!(result.histogram.len(), prices.len());

        // 慢线 EMA 的第一个有效位置是 3 - 1 = 2。
        assert_eq!(result.dif.first_valid_index(), Some(2));

        // DEA 需要积累 signal_period 个 DIF。
        // 第一个有效位置是 2 + 2 - 1 = 3。
        assert_eq!(result.dea.first_valid_index(), Some(3));

        assert_eq!(result.histogram.first_valid_index(), Some(3));
    }

    #[test]
    fn returns_error_when_fast_period_is_not_less_than_slow_period() {
        let prices = [10.0, 11.0, 12.0];

        let result = calculate_macd(&prices, 5, 3, 2);

        assert!(result.is_err());
    }

    #[test]
    fn returns_error_when_period_is_zero() {
        let prices = [10.0, 11.0, 12.0];

        let result = calculate_macd(&prices, 0, 3, 2);

        assert!(result.is_err());
    }

    #[test]
    fn returns_error_for_non_finite_price() {
        let prices = [10.0, 11.0, f64::NAN, 13.0];

        let result = calculate_macd(&prices, 2, 3, 2);

        assert!(result.is_err());
    }
}
