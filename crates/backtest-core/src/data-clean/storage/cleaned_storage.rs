use super::super::cleaner::{CleaningResult, RejectedRecord};
use super::super::quality::DataQualityReport;
use crate::data::Bar;

use serde::Serialize;
use std::error::Error;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct StoragePaths {
    pub bars_path: PathBuf,
    pub rejected_path: PathBuf,
    pub quality_report_path: PathBuf,
}

// CSV 不适合直接保存 Vec<String>，
// 所以把多个拒绝原因合并成一个字符串。
#[derive(Serialize)]
struct RejectedCsvRecord {
    row_number: usize,
    ts_code: String,
    trade_date: String,
    reasons: String,
}

impl From<&RejectedRecord> for RejectedCsvRecord {
    fn from(record: &RejectedRecord) -> Self {
        Self {
            row_number: record.row_number,
            ts_code: record.ts_code.clone(),
            trade_date: record.trade_date.clone(),
            reasons: record.reasons.join("; "),
        }
    }
}

/// 保存一次完整的清洗结果。
///
/// output_dir 是本次数据集的输出目录，
/// 例如 data/cleaned/000001.SZ/20240101_20241231
pub fn save_cleaning_result(
    result: &CleaningResult,
    output_dir: impl AsRef<Path>,
) -> Result<StoragePaths, Box<dyn Error>> {
    let output_dir = output_dir.as_ref();

    // 1. 创建输出目录
    fs::create_dir_all(output_dir)?;

    let bars_path = output_dir.join("bars.csv");
    let rejected_path = output_dir.join("rejected.csv");
    let quality_report_path = output_dir.join("quality_report.json");

    // 2. 保存合法行情
    {
        let mut writer = csv::Writer::from_path(&bars_path)?;

        for bar in &result.bars {
            writer.serialize(bar)?;
        }

        writer.flush()?;
    }

    // 3. 保存被拒绝的记录
    {
        let mut writer = csv::Writer::from_path(&rejected_path)?;

        for record in &result.rejected {
            let csv_record = RejectedCsvRecord::from(record);
            writer.serialize(csv_record)?;
        }

        writer.flush()?;
    }

    // 4. 保存质量报告
    {
        let file = File::create(&quality_report_path)?;
        let mut writer = std::io::BufWriter::new(file);

        serde_json::to_writer_pretty(&mut writer, &result.quality)?;

        use std::io::Write;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }

    Ok(StoragePaths {
        bars_path,
        rejected_path,
        quality_report_path,
    })
}

/// 从已经清洗并保存的 CSV 中重新加载 Bar。
pub fn load_cleaned_bars(path: impl AsRef<Path>) -> Result<Vec<Bar>, Box<dyn Error>> {
    let mut reader = csv::Reader::from_path(path)?;

    let mut bars = Vec::new();

    for row in reader.deserialize() {
        let bar: Bar = row?;
        bars.push(bar);
    }

    Ok(bars)
}

/// 从 JSON 文件读取数据质量报告。
pub fn load_quality_report(path: impl AsRef<Path>) -> Result<DataQualityReport, Box<dyn Error>> {
    let file = File::open(path)?;

    let report = serde_json::from_reader(file)?;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "cleaned_storage_test_{}_{}",
            std::process::id(),
            unique
        ))
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-8,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn saves_and_loads_cleaning_result() {
        let output_dir = create_temp_dir();

        let mut quality = DataQualityReport {
            total_rows: 3,
            accepted_rows: 2,
            rejected_rows: 1,
            duplicate_rows: 0,
            ..Default::default()
        };

        quality.record_warnings(&["zero trading volume".to_string()]);

        let result = CleaningResult {
            bars: vec![
                Bar {
                    symbol: "000001.SZ".to_string(),
                    date: "2024-01-02".to_string(),
                    open: 9.39,
                    high: 9.42,
                    low: 9.21,
                    close: 9.21,
                    volume: 115_836_645.0,
                    amount: Some(1_075_742_252.0),
                },
                Bar {
                    symbol: "000001.SZ".to_string(),
                    date: "2024-01-03".to_string(),
                    open: 9.19,
                    high: 9.22,
                    low: 9.15,
                    close: 9.20,
                    volume: 73_361_031.0,
                    amount: Some(673_673_614.0),
                },
            ],
            rejected: vec![RejectedRecord {
                row_number: 4,
                ts_code: "000001.SZ".to_string(),
                trade_date: "20240104".to_string(),
                reasons: vec!["high is inconsistent with OHLC prices".to_string()],
            }],
            quality,
        };

        // 保存
        let paths = save_cleaning_result(&result, &output_dir).unwrap();

        // 检查三个文件是否存在
        assert!(paths.bars_path.exists());
        assert!(paths.rejected_path.exists());
        assert!(paths.quality_report_path.exists());

        // 重新加载行情
        let loaded_bars = load_cleaned_bars(&paths.bars_path).unwrap();

        assert_eq!(loaded_bars.len(), 2);

        assert_eq!(loaded_bars[0].symbol, "000001.SZ");
        assert_eq!(loaded_bars[0].date, "2024-01-02");
        assert_close(loaded_bars[0].open, 9.39);
        assert_close(loaded_bars[0].high, 9.42);
        assert_close(loaded_bars[0].low, 9.21);
        assert_close(loaded_bars[0].close, 9.21);
        assert_close(loaded_bars[0].volume, 115_836_645.0);
        assert_eq!(loaded_bars[0].amount, Some(1_075_742_252.0));

        assert_eq!(loaded_bars[1].date, "2024-01-03");
        assert_eq!(loaded_bars[1].amount, Some(673_673_614.0));

        // 重新加载质量报告
        let loaded_quality = load_quality_report(&paths.quality_report_path).unwrap();

        assert_eq!(loaded_quality.total_rows, 3);
        assert_eq!(loaded_quality.accepted_rows, 2);
        assert_eq!(loaded_quality.rejected_rows, 1);
        assert_eq!(loaded_quality.warning_rows, 1);

        assert_eq!(
            loaded_quality.warning_reasons.get("zero trading volume"),
            Some(&1)
        );

        // 清理测试目录
        fs::remove_dir_all(&output_dir).unwrap();
    }
}
