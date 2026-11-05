use super::parser::RawTushareBar;

pub fn validate_raw_bar(bar: &RawTushareBar) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    // 1. 股票代码
    if bar.ts_code.trim().is_empty() {
        errors.push("missing stock code".to_string());
    }

    // 2. 日期
    if !is_valid_date(&bar.trade_date) {
        errors.push("invalid trade date".to_string());
    }

    // 3. 必需字段
    let open = check_required("open", bar.open, &mut errors);
    let high = check_required("high", bar.high, &mut errors);
    let low = check_required("low", bar.low, &mut errors);
    let close = check_required("close", bar.close, &mut errors);
    let volume = check_required("vol", bar.vol, &mut errors);

    // 4. 价格必须大于 0
    for (name, value) in [
        ("open", open),
        ("high", high),
        ("low", low),
        ("close", close),
    ] {
        if let Some(value) = value {
            if value <= 0.0 {
                errors.push(format!("{name} must be greater than 0"));
            }
        }
    }

    // 5. OHLC 价格关系
    if let (Some(open), Some(high), Some(low), Some(close)) = (open, high, low, close) {
        if high < open || high < close || high < low {
            errors.push("high is inconsistent with OHLC prices".to_string());
        }

        if low > open || low > close || low > high {
            errors.push("low is inconsistent with OHLC prices".to_string());
        }
    }

    // 6. 成交量不能为负数
    if let Some(volume) = volume {
        if volume < 0.0 {
            errors.push("volume must not be negative".to_string());
        }
    }

    // 7. 可选字段的数值检查
    if let Some(value) = bar.pre_close {
        if !value.is_finite() || value <= 0.0 {
            errors.push("invalid pre_close".to_string());
        }
    }

    if let Some(value) = bar.change {
        if !value.is_finite() {
            errors.push("invalid change".to_string());
        }
    }

    if let Some(value) = bar.pct_chg {
        if !value.is_finite() {
            errors.push("invalid pct_chg".to_string());
        }
    }

    if let Some(value) = bar.amount {
        if !value.is_finite() || value < 0.0 {
            errors.push("invalid amount".to_string());
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// 检查行情中值得关注的异常。
///
/// 这些检查只产生警告，不拒绝行情。
pub fn inspect_market_anomalies(bar: &RawTushareBar) -> Vec<String> {
    let mut warnings = Vec::new();

    // 1. 单日涨跌幅异常
    // pct_chg 是百分数，例如 5.0 表示上涨 5%。
    if let Some(pct_chg) = bar.pct_chg {
        if pct_chg.is_finite() && pct_chg.abs() > 30.0 {
            warnings.push(format!("extreme daily price change: {pct_chg:.2}%"));
        }
    } else if let (Some(close), Some(pre_close)) = (bar.close, bar.pre_close) {
        if close.is_finite() && pre_close.is_finite() && pre_close > 0.0 {
            let change_pct = (close - pre_close) / pre_close * 100.0;

            if change_pct.abs() > 30.0 {
                warnings.push(format!(
                    "extreme daily price change calculated from close and pre_close: {change_pct:.2}%"
                ));
            }
        }
    }

    // 2. 当日最高价与最低价之间的振幅异常
    if let (Some(high), Some(low)) = (bar.high, bar.low) {
        if high.is_finite() && low.is_finite() && low > 0.0 {
            let range_pct = (high - low) / low * 100.0;

            if range_pct > 30.0 {
                warnings.push(format!("extreme intraday price range: {range_pct:.2}%"));
            }
        }
    }

    // 3. 成交量为零
    // 零成交量可能代表特殊交易状态，不直接判定为错误。
    if let Some(volume) = bar.vol {
        if volume == 0.0 {
            warnings.push("zero trading volume".to_string());
        }
    }

    warnings
}

fn check_required(name: &str, value: Option<f64>, errors: &mut Vec<String>) -> Option<f64> {
    match value {
        None => {
            errors.push(format!("missing {name}"));
            None
        }

        Some(value) if !value.is_finite() => {
            errors.push(format!("{name} must be finite"));
            None
        }

        Some(value) => Some(value),
    }
}

fn is_valid_date(date: &str) -> bool {
    if date.len() != 8 {
        return false;
    }

    if !date.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let year = match date[0..4].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return false,
    };

    let month = match date[4..6].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return false,
    };

    let day = match date[6..8].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return false,
    };

    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }

    let leap_year = year % 400 == 0 || (year % 4 == 0 && year % 100 != 0);

    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return false,
    };

    (1..=days_in_month).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_market_anomalies_without_rejecting_bar() {
        let bar = RawTushareBar {
            ts_code: "000001.SZ".to_string(),
            trade_date: "20240102".to_string(),
            open: Some(10.0),
            high: Some(10.0),
            low: Some(10.0),
            close: Some(10.0),
            pre_close: Some(5.0),
            change: Some(5.0),
            pct_chg: Some(100.0),
            vol: Some(0.0),
            amount: Some(0.0),
        };

        // 基础数据仍然可以通过结构校验。
        assert!(validate_raw_bar(&bar).is_ok());

        // 但行情检查会提示异常。
        let warnings = inspect_market_anomalies(&bar);

        assert!(warnings.iter().any(|w| w.contains("price change")));
        assert!(warnings.iter().any(|w| w.contains("zero trading volume")));
    }
}
