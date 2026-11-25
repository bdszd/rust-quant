use crate::data::Bar;
use crate::indicators::core::{Indicator, IndicatorError, IndicatorSeries};

/// 布林带的三条输出线。
#[derive(Debug, Clone, PartialEq)]
pub struct BollingerOutput {
    /// 中轨：移动平均线。
    pub middle: IndicatorSeries,

    /// 上轨：中轨 + 标准差倍数 × 标准差。
    pub upper: IndicatorSeries,

    /// 下轨：中轨 - 标准差倍数 × 标准差。
    pub lower: IndicatorSeries,
}

/// 布林带指标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BollingerBands {
    window: usize,
    multiplier: f64,
}

impl BollingerBands {
    /// 创建布林带指标。
    ///
    /// window：计算窗口，常见为 20。
    /// multiplier：标准差倍数，常见为 2.0。
    pub fn new(window: usize, multiplier: f64) -> Self {
        Self { window, multiplier }
    }

    pub fn window(&self) -> usize {
        self.window
    }

    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }
}

impl Indicator for BollingerBands {
    type Output = BollingerOutput;

    fn calculate(&self, bars: &[Bar]) -> Result<Self::Output, IndicatorError> {
        let prices: Vec<f64> = bars.iter().map(|bar| bar.close).collect();

        calculate_bollinger(&prices, self.window, self.multiplier)
    }
}

/// 根据收盘价序列计算布林带。
fn calculate_bollinger(
    prices: &[f64],
    window: usize,
    multiplier: f64,
) -> Result<BollingerOutput, IndicatorError> {
    if window == 0 {
        return Err(IndicatorError::InvalidPeriod { period: window });
    }

    if !multiplier.is_finite() || multiplier <= 0.0 {
        return Err(IndicatorError::InvalidParameter {
            name: "multiplier",
            value: multiplier,
        });
    }

    let mut middle = vec![None; prices.len()];
    let mut upper = vec![None; prices.len()];
    let mut lower = vec![None; prices.len()];

    // 首先检查输入数据。
    for (index, price) in prices.iter().enumerate() {
        if !price.is_finite() {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "收盘价不是有效的有限数值",
            });
        }
    }

    // 数据不足一个完整窗口时，返回全 None。
    if prices.len() < window {
        return Ok(BollingerOutput {
            middle: IndicatorSeries::new(middle),
            upper: IndicatorSeries::new(upper),
            lower: IndicatorSeries::new(lower),
        });
    }

    // 从第一个完整窗口开始计算。
    for index in (window - 1)..prices.len() {
        let start = index + 1 - window;
        let window_prices = &prices[start..=index];

        // 第一步：计算窗口内的平均价格。
        let sum: f64 = window_prices.iter().sum();

        if !sum.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        let mean = sum / window as f64;

        if !mean.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        // 第二步：计算每个价格与平均价格的差值平方。
        let squared_deviations: Vec<f64> = window_prices
            .iter()
            .map(|price| {
                let difference = price - mean;
                difference * difference
            })
            .collect();

        // 第三步：计算总体方差。
        let variance_sum: f64 = squared_deviations.iter().sum();

        if !variance_sum.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        let variance = variance_sum / window as f64;

        if !variance.is_finite() || variance < 0.0 {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        // 第四步：标准差。
        let standard_deviation = variance.sqrt();

        // 第五步：计算上下轨。
        let band_width = multiplier * standard_deviation;

        let upper_value = mean + band_width;
        let lower_value = mean - band_width;

        if !standard_deviation.is_finite()
            || !band_width.is_finite()
            || !upper_value.is_finite()
            || !lower_value.is_finite()
        {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        middle[index] = Some(mean);
        upper[index] = Some(upper_value);
        lower[index] = Some(lower_value);
    }

    Ok(BollingerOutput {
        middle: IndicatorSeries::new(middle),
        upper: IndicatorSeries::new(upper),
        lower: IndicatorSeries::new(lower),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bar(date: &str, open: f64, high: f64, low: f64, close: f64) -> Bar {
        Bar {
            symbol: "000001.SZ".to_string(),
            date: date.to_string(),
            open,
            high,
            low,
            close,
            volume: 1000.0,
            amount: None,
        }
    }

    #[test]
    fn calculates_bollinger_bands() {
        let bars = vec![
            make_bar("20240101", 10.0, 11.0, 9.0, 10.0),
            make_bar("20240102", 11.0, 12.0, 10.0, 11.0),
            make_bar("20240103", 12.0, 13.0, 11.0, 12.0),
            make_bar("20240104", 13.0, 14.0, 12.0, 13.0),
            make_bar("20240105", 14.0, 15.0, 13.0, 14.0),
        ];

        let indicator = BollingerBands::new(5, 2.0);
        let result = indicator.calculate(&bars).unwrap();

        assert_eq!(result.middle.len(), 5);
        assert_eq!(result.middle.first_valid_index(), Some(4));
        assert_eq!(result.upper.first_valid_index(), Some(4));
        assert_eq!(result.lower.first_valid_index(), Some(4));

        assert_eq!(result.middle.get(0), None);
        assert_eq!(result.middle.get(3), None);

        let middle = result.middle.get(4).unwrap();
        let upper = result.upper.get(4).unwrap();
        let lower = result.lower.get(4).unwrap();

        assert!((middle - 12.0).abs() < 1e-10);
        assert!(upper > middle);
        assert!(lower < middle);
    }

    #[test]
    fn returns_none_when_data_is_insufficient() {
        let bars = vec![
            make_bar("20240101", 10.0, 11.0, 9.0, 10.0),
            make_bar("20240102", 11.0, 12.0, 10.0, 11.0),
        ];

        let indicator = BollingerBands::new(5, 2.0);
        let result = indicator.calculate(&bars).unwrap();

        assert_eq!(result.middle.first_valid_index(), None);
        assert_eq!(result.upper.first_valid_index(), None);
        assert_eq!(result.lower.first_valid_index(), None);
    }

    #[test]
    fn rejects_zero_window() {
        let indicator = BollingerBands::new(0, 2.0);

        let result = indicator.calculate(&[]);

        assert!(matches!(
            result,
            Err(IndicatorError::InvalidPeriod { period: 0 })
        ));
    }

    #[test]
    fn rejects_invalid_multiplier() {
        let indicator = BollingerBands::new(5, 0.0);

        let result = indicator.calculate(&[]);

        assert!(matches!(
            result,
            Err(IndicatorError::InvalidParameter { .. })
        ));
    }

    #[test]
    fn rejects_nan_close() {
        let bars = vec![make_bar("20240101", 10.0, 11.0, 9.0, f64::NAN)];

        let indicator = BollingerBands::new(5, 2.0);
        let result = indicator.calculate(&bars);

        assert!(matches!(
            result,
            Err(IndicatorError::InvalidInput { index: 0, .. })
        ));
    }

    #[test]
    fn flat_prices_produce_identical_bands() {
        let bars = vec![
            make_bar("20240101", 10.0, 10.0, 10.0, 10.0),
            make_bar("20240102", 10.0, 10.0, 10.0, 10.0),
            make_bar("20240103", 10.0, 10.0, 10.0, 10.0),
        ];

        let indicator = BollingerBands::new(3, 2.0);
        let result = indicator.calculate(&bars).unwrap();

        assert_eq!(result.middle.get(2), Some(10.0));
        assert_eq!(result.upper.get(2), Some(10.0));
        assert_eq!(result.lower.get(2), Some(10.0));
    }
}
