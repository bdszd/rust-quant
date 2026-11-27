use crate::domain::{AmountUnit, DailyBar, RawDailyBar, VolumeUnit};
use rust_decimal::{Decimal, prelude::ToPrimitive};

pub fn normalize_date(value: &str) -> Result<String, String> {
    let compact = value.replace('-', "");
    if compact.len() != 8 || !compact.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("日期格式错误：{value}"));
    }
    let y: u32 = compact[0..4]
        .parse()
        .map_err(|_| format!("日期错误：{value}"))?;
    let m: u32 = compact[4..6]
        .parse()
        .map_err(|_| format!("日期错误：{value}"))?;
    let d: u32 = compact[6..8]
        .parse()
        .map_err(|_| format!("日期错误：{value}"))?;
    let leap = y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let max = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if d == 0 || d > max {
        return Err(format!("非法日期：{value}"));
    }
    Ok(format!(
        "{}-{}-{}",
        &compact[0..4],
        &compact[4..6],
        &compact[6..8]
    ))
}

pub fn normalize_daily_bar(raw: RawDailyBar) -> Result<DailyBar, String> {
    let symbol = crate::domain::Symbol::new(raw.symbol)?;
    let trade_date = normalize_date(&raw.trade_date)?;
    let volume_in_shares = match raw.volume_unit {
        VolumeUnit::Lots => raw.volume * Decimal::from(100),
        VolumeUnit::Shares => raw.volume,
    };

    // 成交量最终以“股”为单位，必须是整数；不能静默截断小数。
    let volume = volume_in_shares
        .to_i64()
        .ok_or_else(|| "成交量不是有效整数或超出范围".to_owned())?;
    if Decimal::from(volume) != volume_in_shares {
        return Err("成交量换算成股后必须是整数".to_owned());
    }

    let amount = raw.amount.map(|v| match raw.amount_unit {
        Some(AmountUnit::ThousandCny) => v * Decimal::from(1000),
        _ => v,
    });
    Ok(DailyBar {
        symbol,
        trade_date,
        open: raw.open,
        high: raw.high,
        low: raw.low,
        close: raw.close,
        volume,
        amount,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AmountUnit, DatasetSource, PriceUnit, RawDailyBar, VolumeUnit};
    use serde_json::json;
    use std::str::FromStr;

    fn decimal(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn raw_bar(volume: &str, volume_unit: VolumeUnit) -> RawDailyBar {
        RawDailyBar {
            symbol: "000001.SZ".into(),
            trade_date: "20240102".into(),
            open: decimal("10"),
            high: decimal("11"),
            low: decimal("9"),
            close: decimal("10.5"),
            volume: decimal(volume),
            amount: Some(decimal("4567.89")),
            price_unit: PriceUnit::Cny,
            volume_unit,
            amount_unit: Some(AmountUnit::ThousandCny),
            source: DatasetSource::Tushare,
            raw_payload: json!({
                "ts_code": "000001.SZ",
                "trade_date": "20240102",
                "vol": volume
            }),
        }
    }

    #[test]
    fn converts_tushare_volume_and_amount_units() {
        let bar = normalize_daily_bar(raw_bar("123.45", VolumeUnit::Lots)).unwrap();

        assert_eq!(bar.volume, 12345);
        assert_eq!(bar.amount, Some(decimal("4567890")));
        assert_eq!(bar.trade_date, "2024-01-02");
    }

    #[test]
    fn rejects_fractional_share_volume() {
        let result = normalize_daily_bar(raw_bar("123.45", VolumeUnit::Shares));

        assert!(result.is_err());
    }

    #[test]
    fn rejects_lot_volume_that_converts_to_fractional_shares() {
        let result = normalize_daily_bar(raw_bar("123.456", VolumeUnit::Lots));

        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_date() {
        assert!(normalize_date("20240230").is_err());
    }
}
