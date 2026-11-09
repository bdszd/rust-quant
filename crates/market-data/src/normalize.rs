use crate::domain::{AmountUnit, DailyBar, PriceUnit, RawDailyBar, VolumeUnit};
use rust_decimal::{Decimal, prelude::ToPrimitive};

pub fn normalize_date(value: &str) -> Result<String, String> {
    let compact = value.replace('-', "");
    if compact.len() != 8 || !compact.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("日期格式错误：{value}"));
    }
    let y: u32 = compact[0..4].parse().map_err(|_| format!("日期错误：{value}"))?;
    let m: u32 = compact[4..6].parse().map_err(|_| format!("日期错误：{value}"))?;
    let d: u32 = compact[6..8].parse().map_err(|_| format!("日期错误：{value}"))?;
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let max = match m { 1|3|5|7|8|10|12 => 31, 4|6|9|11 => 30, 2 if leap => 29, 2 => 28, _ => 0 };
    if d == 0 || d > max { return Err(format!("非法日期：{value}")); }
    Ok(format!("{}-{}-{}", &compact[0..4], &compact[4..6], &compact[6..8]))
}

pub fn normalize_daily_bar(raw: RawDailyBar) -> Result<DailyBar, String> {
    let symbol = crate::domain::Symbol::new(raw.symbol)?;
    let trade_date = normalize_date(&raw.trade_date)?;
    let _ = match raw.price_unit { PriceUnit::Cny => () };
    let volume = match raw.volume_unit {
        VolumeUnit::Lots => raw.volume * Decimal::from(100),
        VolumeUnit::Shares => raw.volume,
    };
    let volume = volume.to_i64().ok_or_else(|| "成交量不是有效整数或超出范围".to_owned())?;
    let amount = raw.amount.map(|v| match raw.amount_unit {
        Some(AmountUnit::ThousandCny) => v * Decimal::from(1000),
        _ => v,
    });
    Ok(DailyBar { symbol, trade_date, open: raw.open, high: raw.high, low: raw.low,
        close: raw.close, volume, amount })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AmountUnit, PriceUnit, RawDailyBar, VolumeUnit};
    use std::str::FromStr;
    #[test]
    fn converts_tushare_volume_and_amount_units() {
        let d = |v| Decimal::from_str(v).unwrap();
        let bar = normalize_daily_bar(RawDailyBar {
            symbol: "000001.SZ".into(), trade_date: "20240102".into(),
            open: d("10"), high: d("11"), low: d("9"), close: d("10.5"),
            volume: d("123.45"), amount: Some(d("4567.89")),
            price_unit: PriceUnit::Cny, volume_unit: VolumeUnit::Lots,
            amount_unit: Some(AmountUnit::ThousandCny),
        }).unwrap();
        assert_eq!(bar.volume, 12345);
        assert_eq!(bar.amount, Some(d("4567890")));
        assert_eq!(bar.trade_date, "2024-01-02");
    }
    #[test]
    fn rejects_invalid_date() { assert!(normalize_date("20240230").is_err()); }
}
