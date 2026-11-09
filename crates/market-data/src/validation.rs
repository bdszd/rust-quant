use crate::domain::DailyBar;
use rust_decimal::Decimal;

pub fn validate_daily_bar(bar: &DailyBar) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if [bar.open, bar.high, bar.low, bar.close].iter().any(|v| *v <= Decimal::ZERO) {
        errors.push("OHLC 价格必须大于 0".to_owned());
    }
    if bar.high < bar.open || bar.high < bar.low || bar.high < bar.close {
        errors.push("high 不能低于 open、low 或 close".to_owned());
    }
    if bar.low > bar.open || bar.low > bar.high || bar.low > bar.close {
        errors.push("low 不能高于 open、high 或 close".to_owned());
    }
    if bar.volume < 0 { errors.push("volume 不能为负数".to_owned()); }
    if bar.amount.is_some_and(|v| v < Decimal::ZERO) {
        errors.push("amount 不能为负数".to_owned());
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}
