/// 计算 prices 中最后 window 个价格的简单移动平均线。
///
/// 如果价格数量不足，或者 window 为 0，则返回 None。
pub fn sma(prices: &[f64], window: usize) -> Option<f64> {
    if window == 0 || prices.len() < window {
        return None;
    }

    let start = prices.len() - window;
    let sum: f64 = prices[start..].iter().sum();

    Some(sum / window as f64)
}

#[cfg(test)]
mod tests {
    use super::sma;

    #[test]
    fn calculates_simple_moving_average() {
        assert_eq!(sma(&[1.0, 2.0, 3.0], 2), Some(2.5));
    }

    #[test]
    fn returns_none_when_data_is_insufficient() {
        assert_eq!(sma(&[1.0, 2.0], 3), None);
    }

    #[test]
    fn returns_none_for_zero_window() {
        assert_eq!(sma(&[1.0, 2.0], 0), None);
    }
}
