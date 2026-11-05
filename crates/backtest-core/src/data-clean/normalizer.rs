use super::parser::RawTushareBar;
use crate::data::Bar;

/// 将 Tushare 原始行情转换为系统内部统一格式。
///
/// Tushare daily:
/// - open/high/low/close: 元
/// - vol: 手
/// - amount: 千元
///
/// 内部标准:
/// - open/high/low/close: 元
/// - volume: 股
/// - amount: 元
pub fn normalize_bar(raw: &RawTushareBar) -> Bar {
    let date = format!(
        "{}-{}-{}",
        &raw.trade_date[0..4],
        &raw.trade_date[4..6],
        &raw.trade_date[6..8],
    );

    Bar {
        symbol: raw.ts_code.clone(),

        date,

        open: raw.open.expect("validated open"),
        high: raw.high.expect("validated high"),
        low: raw.low.expect("validated low"),
        close: raw.close.expect("validated close"),

        // Tushare 的成交量单位是手。
        // 1 手 = 100 股。
        volume: raw.vol.expect("validated vol") * 100.0,

        // Tushare 的成交额单位是千元。
        // 1 千元 = 1000 元。
        amount: raw.amount.map(|value| value * 1000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_tushare_units() {
        let raw = RawTushareBar {
            ts_code: "000001.SZ".to_string(),
            trade_date: "20240102".to_string(),
            open: Some(9.39),
            high: Some(9.42),
            low: Some(9.21),
            close: Some(9.21),
            pre_close: Some(9.39),
            change: Some(-0.18),
            pct_chg: Some(-1.9169),
            vol: Some(100.0),
            amount: Some(200.0),
        };

        let bar = normalize_bar(&raw);

        assert_eq!(bar.symbol, "000001.SZ");
        assert_eq!(bar.date, "2024-01-02");

        assert_eq!(bar.volume, 10_000.0);
        assert_eq!(bar.amount, Some(200_000.0));
    }
}
