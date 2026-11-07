use serde::{Deserialize, Serialize};
use std::{collections::HashSet, error::Error, fmt};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bar {
    #[serde(default, alias = "ts_code")]
    pub symbol: String,
    #[serde(alias = "trade_date")]
    pub date: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    #[serde(default, alias = "vol")]
    pub volume: f64,
    #[serde(default)]
    pub amount: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataValidationError {
    pub row: usize,
    pub message: String,
}

impl fmt::Display for DataValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "第 {} 行：{}", self.row, self.message)
    }
}

impl Error for DataValidationError {}

/// 读取 CSV 行情。兼容标准字段以及 Tushare 的 ts_code/trade_date/vol。
/// 读取后按日期升序排列，并拒绝重复日期、非法 OHLC 和无效成交量。
pub fn load_bars(path: &str) -> Result<Vec<Bar>, Box<dyn Error>> {
    let mut reader = csv::Reader::from_path(path)?;
    let mut bars = Vec::new();

    for row in reader.deserialize() {
        let bar: Bar = row?;
        bars.push(bar);
    }

    validate_bars(&bars)?;
    bars.sort_by(|a, b| a.date.cmp(&b.date));
    validate_bars(&bars)?;
    Ok(bars)
}

pub fn validate_bars(bars: &[Bar]) -> Result<(), DataValidationError> {
    let mut dates = HashSet::with_capacity(bars.len());
    let mut symbol: Option<&str> = None;

    for (index, bar) in bars.iter().enumerate() {
        let row = index + 2;
        if bar.date.trim().is_empty() {
            return Err(DataValidationError { row, message: "交易日期不能为空".into() });
        }
        if !dates.insert(bar.date.as_str()) {
            return Err(DataValidationError { row, message: format!("重复交易日期：{}", bar.date) });
        }
        if let Some(expected) = symbol {
            if !bar.symbol.is_empty() && !expected.is_empty() && bar.symbol != expected {
                return Err(DataValidationError { row, message: "单次回测数据集只能包含一个股票代码".into() });
            }
        } else {
            symbol = Some(&bar.symbol);
        }
        let prices = [bar.open, bar.high, bar.low, bar.close];
        if prices.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(DataValidationError { row, message: "OHLC 必须是有限的正数".into() });
        }
        if bar.low > bar.open.min(bar.close) || bar.high < bar.open.max(bar.close) || bar.low > bar.high {
            return Err(DataValidationError { row, message: "OHLC 价格关系不合法".into() });
        }
        if !bar.volume.is_finite() || bar.volume < 0.0 {
            return Err(DataValidationError { row, message: "成交量必须是有限的非负数".into() });
        }
        if let Some(amount) = bar.amount {
            if !amount.is_finite() || amount < 0.0 {
                return Err(DataValidationError { row, message: "成交额必须是有限的非负数".into() });
            }
        }
    }

    Ok(())
}
