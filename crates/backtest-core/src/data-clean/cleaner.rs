use std::collections::HashSet;
use std::error::Error;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::data::Bar;

use super::normalizer::normalize_bar;
use super::parser::{RawTushareBar, parse_tushare_csv};
use super::quality::DataQualityReport;
use super::validator::{inspect_market_anomalies, validate_raw_bar};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedRecord {
    pub row_number: usize,
    pub ts_code: String,
    pub trade_date: String,
    pub reasons: Vec<String>,
}

#[derive(Debug)]
pub struct CleaningResult {
    pub bars: Vec<Bar>,
    pub rejected: Vec<RejectedRecord>,
    pub quality: DataQualityReport,
}

pub fn clean_tushare_file(path: impl AsRef<Path>) -> Result<CleaningResult, Box<dyn Error>> {
    // 1. 逐条解析 CSV。
    let rows = parse_tushare_csv(path)?;

    let mut quality = DataQualityReport {
        total_rows: rows.len(),
        ..Default::default()
    };

    let mut clean_bars = Vec::new();
    let mut rejected = Vec::new();

    let mut seen_dates: HashSet<(String, String)> = HashSet::new();

    // 2. 逐条处理。
    for row in rows {
        let row_number = row.row_number;

        // 2.1 处理 Parser 阶段的错误。
        let raw = match row.raw {
            Some(raw) => raw,

            None => {
                let reasons = vec![format!(
                    "parse error: {}",
                    row.parse_error
                        .unwrap_or_else(|| "unknown error".to_string())
                )];

                quality.record_rejection(&reasons);

                rejected.push(RejectedRecord {
                    row_number,
                    ts_code: row.ts_code,
                    trade_date: row.trade_date,
                    reasons,
                });

                continue;
            }
        };

        // 2.2 基础数据校验。
        if let Err(reasons) = validate_raw_bar(&raw) {
            quality.record_rejection(&reasons);

            rejected.push(make_rejected_record(row_number, &raw, reasons));

            continue;
        }

        // 2.3 行情异常检查。
        // 这里只产生警告，不会拒绝数据。
        let warnings = inspect_market_anomalies(&raw);
        quality.record_warnings(&warnings);

        // 2.4 标准化。
        let bar = normalize_bar(&raw);

        // 2.5 检查重复。
        let key = (bar.symbol.clone(), bar.date.clone());

        if !seen_dates.insert(key) {
            let reasons = vec!["duplicate trade date".to_string()];

            quality.record_rejection(&reasons);

            rejected.push(make_rejected_record(row_number, &raw, reasons));

            continue;
        }

        // 2.6 接受合法数据。
        quality.accepted_rows += 1;

        clean_bars.push(bar);
    }

    // 3. 按股票代码、交易日期排序。
    clean_bars.sort_by(|a, b| a.symbol.cmp(&b.symbol).then_with(|| a.date.cmp(&b.date)));

    Ok(CleaningResult {
        bars: clean_bars,
        rejected,
        quality,
    })
}

fn make_rejected_record(
    row_number: usize,
    raw: &RawTushareBar,
    reasons: Vec<String>,
) -> RejectedRecord {
    RejectedRecord {
        row_number,
        ts_code: raw.ts_code.clone(),
        trade_date: raw.trade_date.clone(),
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_temp_csv(csv_data: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let path = std::env::temp_dir().join(format!(
            "tushare_test_{}_{}.csv",
            std::process::id(),
            unique
        ));

        fs::write(&path, csv_data).unwrap();

        path
    }

    #[test]
    fn cleans_tushare_csv() {
        let csv_data = concat!(
            "ts_code,trade_date,open,high,low,close,pre_close,change,pct_chg,vol,amount\n",
            "000001.SZ,20240103,9.19,9.22,9.15,9.20,9.21,-0.01,-0.1086,733610.31,673673.614\n",
            "000001.SZ,20240102,9.39,9.42,9.21,9.21,9.39,-0.18,-1.9169,1158366.45,1075742.252\n",
            "000001.SZ,20240102,9.39,9.42,9.21,9.21,9.39,-0.18,-1.9169,1158366.45,1075742.252\n",
            "000001.SZ,20240104,9.19,9.10,9.08,9.11,9.20,-0.09,-0.9783,864193.99,787470.082\n",
        );

        let path = write_temp_csv(csv_data);

        let result = clean_tushare_file(&path).unwrap();

        fs::remove_file(&path).unwrap();

        assert_eq!(result.quality.total_rows, 4);
        assert_eq!(result.quality.accepted_rows, 2);
        assert_eq!(result.quality.rejected_rows, 2);
        assert_eq!(result.quality.duplicate_rows, 1);

        assert_eq!(result.bars[0].symbol, "000001.SZ");
        assert_eq!(result.bars[0].date, "2024-01-02");
        assert_eq!(result.bars[1].date, "2024-01-03");

        assert_eq!(result.bars[0].volume, 115836645.0);
        assert_eq!(result.bars[0].amount, Some(1_075_742_252.0));

        assert_eq!(result.rejected.len(), 2);
    }

    #[test]
    fn continues_after_one_row_has_invalid_number() {
        let csv_data = concat!(
            "ts_code,trade_date,open,high,low,close,pre_close,change,pct_chg,vol,amount\n",
            "000001.SZ,20240102,9.39,9.42,9.21,9.21,9.39,-0.18,-1.9169,100,200\n",
            "000001.SZ,20240103,9.39,9.42,9.21,abc,9.39,-0.18,-1.9169,100,200\n",
            "000001.SZ,20240104,9.39,9.42,9.21,9.21,9.39,-0.18,-1.9169,100,200\n",
        );

        let path = write_temp_csv(csv_data);
        let result = clean_tushare_file(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(result.quality.total_rows, 3);
        assert_eq!(result.quality.accepted_rows, 2);
        assert_eq!(result.quality.rejected_rows, 1);

        assert_eq!(result.bars.len(), 2);
        assert_eq!(result.rejected.len(), 1);
        assert_eq!(result.rejected[0].row_number, 3);

        assert!(result.rejected[0].reasons[0].contains("parse error"));
    }

    #[test]
    fn deduplicates_by_symbol_and_date() {
        let csv_data = concat!(
            "ts_code,trade_date,open,high,low,close,pre_close,change,pct_chg,vol,amount\n",
            "000001.SZ,20240102,9.39,9.42,9.21,9.21,9.39,-0.18,-1.9169,100,200\n",
            "600000.SH,20240102,9.39,9.42,9.21,9.21,9.39,-0.18,-1.9169,100,200\n",
        );

        let path = write_temp_csv(csv_data);
        let result = clean_tushare_file(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(result.quality.accepted_rows, 2);
        assert_eq!(result.bars.len(), 2);
        assert_ne!(result.bars[0].symbol, result.bars[1].symbol);
    }

    #[test]
    fn market_anomaly_is_warning_not_rejection() {
        let csv_data = concat!(
            "ts_code,trade_date,open,high,low,close,pre_close,change,pct_chg,vol,amount\n",
            "000001.SZ,20240102,10,10,10,10,5,5,100,0,0\n",
        );

        let path = write_temp_csv(csv_data);
        let result = clean_tushare_file(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(result.quality.accepted_rows, 1);
        assert_eq!(result.quality.rejected_rows, 0);
        assert_eq!(result.quality.warning_rows, 1);
        assert!(!result.bars.is_empty());
    }

    #[test]
    fn rejects_invalid_ohlc_record() {
        let csv_data = concat!(
            "ts_code,trade_date,open,high,low,close,pre_close,change,pct_chg,vol,amount\n",
            "000001.SZ,20240102,9.39,9.10,9.08,9.11,9.20,-0.09,-0.9783,100,200\n",
        );

        let path = write_temp_csv(csv_data);
        let result = clean_tushare_file(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(result.quality.accepted_rows, 0);
        assert_eq!(result.quality.rejected_rows, 1);
    }
}
