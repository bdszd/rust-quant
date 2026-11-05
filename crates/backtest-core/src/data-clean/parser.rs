use serde::Deserialize;
use std::error::Error;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct RawTushareBar {
    #[serde(default)]
    pub ts_code: String,

    #[serde(default)]
    pub trade_date: String,

    #[serde(default)]
    pub open: Option<f64>,

    #[serde(default)]
    pub high: Option<f64>,

    #[serde(default)]
    pub low: Option<f64>,

    #[serde(default)]
    pub close: Option<f64>,

    #[serde(default)]
    pub pre_close: Option<f64>,

    #[serde(default)]
    pub change: Option<f64>,

    #[serde(default)]
    pub pct_chg: Option<f64>,

    #[serde(default)]
    pub vol: Option<f64>,

    #[serde(default)]
    pub amount: Option<f64>,
}

/// 一条 CSV 数据的解析结果。
///
/// raw 为 Some：解析成功。
/// raw 为 None：解析失败。
#[derive(Debug)]
pub struct ParsedTushareRow {
    pub row_number: usize,
    pub ts_code: String,
    pub trade_date: String,
    pub raw: Option<RawTushareBar>,
    pub parse_error: Option<String>,
}

/// 逐条解析 Tushare CSV。
///
/// 普通字段类型错误、可恢复的 CSV 记录错误，
/// 都会作为 ParsedTushareRow 返回。
///
/// 注意：row_number 是 CSV 逻辑记录序号（包含 header），
/// 不保证等于原始文件的物理行号。
pub fn parse_tushare_csv(path: impl AsRef<Path>) -> Result<Vec<ParsedTushareRow>, Box<dyn Error>> {
    let mut reader = csv::ReaderBuilder::new()
        // 允许记录字段数量不一致，具体缺失字段交给后续校验。
        .flexible(true)
        .from_path(path)?;

    let headers = reader.headers()?.clone();

    let ts_code_index = headers.iter().position(|name| name == "ts_code");
    let trade_date_index = headers.iter().position(|name| name == "trade_date");

    let mut rows = Vec::new();

    for (index, record_result) in reader.records().enumerate() {
        let row_number = index + 2;

        let record = match record_result {
            Ok(record) => record,

            Err(error) => {
                // CSV 语法错误：尽可能记录并继续读取。
                // 如果错误导致后续记录边界无法恢复，csv 库仍可能无法继续。
                rows.push(ParsedTushareRow {
                    row_number,
                    ts_code: String::new(),
                    trade_date: String::new(),
                    raw: None,
                    parse_error: Some(format!("CSV syntax error: {error}")),
                });

                continue;
            }
        };

        let ts_code = ts_code_index
            .and_then(|i| record.get(i))
            .unwrap_or("")
            .to_string();

        let trade_date = trade_date_index
            .and_then(|i| record.get(i))
            .unwrap_or("")
            .to_string();

        match record.deserialize::<RawTushareBar>(Some(&headers)) {
            Ok(raw) => {
                rows.push(ParsedTushareRow {
                    row_number,
                    ts_code,
                    trade_date,
                    raw: Some(raw),
                    parse_error: None,
                });
            }

            Err(error) => {
                rows.push(ParsedTushareRow {
                    row_number,
                    ts_code,
                    trade_date,
                    raw: None,
                    parse_error: Some(format!("field parse error: {error}")),
                });
            }
        }
    }

    Ok(rows)
}
