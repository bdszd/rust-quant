use crate::data::Bar;

use crate::indicators::core::{Indicator, IndicatorError, IndicatorSeries};

/// 简单移动平均线（Simple Moving Average）。
///
/// window 表示计算窗口大小。
///
/// 例如 SMA(5) 表示当前 K 线及之前 4 根 K 线的收盘价平均值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sma {
    window: usize,
}

impl Sma {
    /// 创建一个 SMA 指标。
    pub fn new(window: usize) -> Self {
        Self { window }
    }

    /// 获取 SMA 的计算窗口。
    pub fn window(&self) -> usize {
        self.window
    }
}

impl Indicator for Sma {
    type Output = IndicatorSeries;

    /// 根据完整的 K 线数据计算 SMA 序列。
    fn calculate(&self, bars: &[Bar]) -> Result<Self::Output, IndicatorError> {
        if self.window == 0 {
            return Err(IndicatorError::InvalidPeriod {
                period: self.window,
            });
        }

        let mut values = vec![None; bars.len()];
        let mut sum = 0.0;

        for (index, bar) in bars.iter().enumerate() {
            let close = bar.close;

            if !close.is_finite() {
                return Err(IndicatorError::InvalidInput {
                    index,
                    reason: "收盘价不是有效的有限数值",
                });
            }

            // 将当前 K 线的收盘价加入窗口。
            sum += close;

            // 如果窗口已经超过指定长度，
            // 就减去窗口最前面那根 K 线的收盘价。
            if index >= self.window {
                sum -= bars[index - self.window].close;
            }

            if !sum.is_finite() {
                return Err(IndicatorError::NonFiniteOutput { index });
            }

            // 数据数量达到窗口要求后，才产生有效指标值。
            if index + 1 >= self.window {
                let value = sum / self.window as f64;

                if !value.is_finite() {
                    return Err(IndicatorError::NonFiniteOutput { index });
                }

                values[index] = Some(value);
            }
        }

        Ok(IndicatorSeries::new(values))
    }
}
