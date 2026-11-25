use crate::data::Bar;
use crate::indicators::core::{Indicator, IndicatorError, IndicatorSeries};

/// Average True Range（平均真实波幅）。
///
/// ATR 衡量市场的波动幅度，不代表价格上涨或下跌的方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Atr {
    period: usize,
}

impl Atr {
    pub fn new(period: usize) -> Self {
        Self { period }
    }

    pub fn period(&self) -> usize {
        self.period
    }
}

impl Indicator for Atr {
    type Output = IndicatorSeries;

    fn calculate(&self, bars: &[Bar]) -> Result<Self::Output, IndicatorError> {
        let values = calculate_atr(bars, self.period)?;
        Ok(IndicatorSeries::new(values))
    }
}

/// 根据 K 线计算 ATR。
///
/// 约定：
/// - 第一根 K 线没有前一日收盘价，因此没有 TR。
/// - period 个 TR 累积完成后，产生第一个 ATR。
/// - 后续 ATR 使用 Wilder 平滑方法。
fn calculate_atr(bars: &[Bar], period: usize) -> Result<Vec<Option<f64>>, IndicatorError> {
    if period == 0 {
        return Err(IndicatorError::InvalidPeriod { period });
    }

    let mut values = vec![None; bars.len()];

    // 至少需要 period + 1 根 K 线：
    // 第一根提供前一日收盘价，后面 period 根提供 TR。
    if bars.len() < period + 1 {
        validate_bars(bars)?;
        return Ok(values);
    }

    validate_bars(bars)?;

    // tr_values[0] 不存在，因为第一根 K 线没有前一日收盘价。
    let mut tr_values = vec![None; bars.len()];

    for index in 1..bars.len() {
        let current = &bars[index];
        let previous = &bars[index - 1];

        let high_low = current.high - current.low;
        let high_previous_close = (current.high - previous.close).abs();
        let low_previous_close = (current.low - previous.close).abs();

        let true_range = high_low.max(high_previous_close).max(low_previous_close);

        if !true_range.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        tr_values[index] = Some(true_range);
    }

    // 使用前 period 个 TR 的算术平均值初始化 ATR。
    //
    // TR 的索引范围是 1..=period。
    let mut initial_sum = 0.0;

    for index in 1..=period {
        let true_range = tr_values[index].expect("该位置应该已经计算出 TR");
        initial_sum += true_range;

        if !initial_sum.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }
    }

    let mut previous_atr = initial_sum / period as f64;

    if !previous_atr.is_finite() {
        return Err(IndicatorError::NonFiniteOutput { index: period });
    }

    // 第一个有效 ATR 位于索引 period。
    values[period] = Some(previous_atr);

    // 后续使用 Wilder smoothing。
    for index in (period + 1)..bars.len() {
        let true_range = tr_values[index].expect("该位置应该已经计算出 TR");

        let current_atr = (previous_atr * (period - 1) as f64 + true_range) / period as f64;

        if !current_atr.is_finite() {
            return Err(IndicatorError::NonFiniteOutput { index });
        }

        values[index] = Some(current_atr);
        previous_atr = current_atr;
    }

    Ok(values)
}

/// 检查计算 ATR 所需的 K 线数据。
fn validate_bars(bars: &[Bar]) -> Result<(), IndicatorError> {
    for (index, bar) in bars.iter().enumerate() {
        if !bar.high.is_finite() {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "最高价不是有效的有限数值",
            });
        }

        if !bar.low.is_finite() {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "最低价不是有效的有限数值",
            });
        }

        if !bar.close.is_finite() {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "收盘价不是有效的有限数值",
            });
        }

        if bar.high < bar.low {
            return Err(IndicatorError::InvalidInput {
                index,
                reason: "最高价不能低于最低价",
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Bar;

    fn make_bar(high: f64, low: f64, close: f64) -> Bar {
        Bar {
            date: "2024-01-01".to_string(),
            symbol: "TEST".to_string(),
            open: close,
            high,
            low,
            close,
            volume: 1000.0,
            amount: None,
        }
    }

    #[test]
    fn calculates_initial_atr() {
        let bars = vec![
            make_bar(11.0, 9.0, 10.0),
            make_bar(12.0, 10.0, 11.0),
            make_bar(15.0, 13.0, 14.0),
        ];

        let atr = Atr::new(2);
        let result = atr.calculate(&bars).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result.get(0), None);
        assert_eq!(result.get(1), None);
        assert_eq!(result.get(2), Some(3.0));
        assert_eq!(result.first_valid_index(), Some(2));
    }

    #[test]
    fn calculates_wilder_smoothing() {
        let bars = vec![
            make_bar(11.0, 9.0, 10.0),
            make_bar(12.0, 10.0, 11.0),
            make_bar(15.0, 13.0, 14.0),
            make_bar(15.0, 13.0, 14.0),
        ];

        let atr = Atr::new(2);
        let result = atr.calculate(&bars).unwrap();

        // TR 分别为：2、4、2。
        // 第一个 ATR = (2 + 4) / 2 = 3。
        // 第二个 ATR = (3 * 1 + 2) / 2 = 2.5。
        assert_eq!(result.get(2), Some(3.0));
        assert_eq!(result.get(3), Some(2.5));
    }

    #[test]
    fn true_range_accounts_for_price_gaps() {
        let bars = vec![make_bar(11.0, 9.0, 10.0), make_bar(12.0, 11.0, 11.5)];

        let atr = Atr::new(1);
        let result = atr.calculate(&bars).unwrap();

        // 当天最高价与最低价之差为 1。
        // 但当天最高价与前一天收盘价之差为 2。
        // 所以 TR = 2。
        assert_eq!(result.get(1), Some(2.0));
    }

    #[test]
    fn returns_none_when_data_is_insufficient() {
        let bars = vec![make_bar(11.0, 9.0, 10.0), make_bar(12.0, 10.0, 11.0)];

        let atr = Atr::new(2);
        let result = atr.calculate(&bars).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result.first_valid_index(), None);
    }

    #[test]
    fn rejects_zero_period() {
        let atr = Atr::new(0);
        let result = atr.calculate(&[]);

        assert_eq!(result, Err(IndicatorError::InvalidPeriod { period: 0 }));
    }

    #[test]
    fn rejects_invalid_price_range() {
        let bars = vec![make_bar(9.0, 11.0, 10.0), make_bar(12.0, 10.0, 11.0)];

        let atr = Atr::new(1);
        let result = atr.calculate(&bars);

        assert!(matches!(
            result,
            Err(IndicatorError::InvalidInput { index: 0, .. })
        ));
    }

    #[test]
    fn rejects_nan_input() {
        let bars = vec![make_bar(11.0, 9.0, 10.0), make_bar(f64::NAN, 10.0, 11.0)];

        let atr = Atr::new(1);
        let result = atr.calculate(&bars);

        assert!(matches!(
            result,
            Err(IndicatorError::InvalidInput { index: 1, .. })
        ));
    }
}
