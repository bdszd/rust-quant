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
            return Err(DataValidationError {
                row,
                message: "交易日期不能为空".into(),
            });
        }
        if !dates.insert(bar.date.as_str()) {
            return Err(DataValidationError {
                row,
                message: format!("重复交易日期：{}", bar.date),
            });
        }
        if let Some(expected) = symbol {
            if !bar.symbol.is_empty() && !expected.is_empty() && bar.symbol != expected {
                return Err(DataValidationError {
                    row,
                    message: "单次回测数据集只能包含一个股票代码".into(),
                });
            }
        } else {
            symbol = Some(&bar.symbol);
        }
        let prices = [bar.open, bar.high, bar.low, bar.close];
        if prices.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(DataValidationError {
                row,
                message: "OHLC 必须是有限的正数".into(),
            });
        }
        if bar.low > bar.open.min(bar.close)
            || bar.high < bar.open.max(bar.close)
            || bar.low > bar.high
        {
            return Err(DataValidationError {
                row,
                message: "OHLC 价格关系不合法".into(),
            });
        }
        if !bar.volume.is_finite() || bar.volume < 0.0 {
            return Err(DataValidationError {
                row,
                message: "成交量必须是有限的非负数".into(),
            });
        }
        if let Some(amount) = bar.amount
            && (!amount.is_finite() || amount < 0.0)
        {
            return Err(DataValidationError {
                row,
                message: "成交额必须是有限的非负数".into(),
            });
        }
    }

    Ok(())
}


/// 多标的行情集合。bars_by_date 以交易日为第一层索引，
/// 同一天只包含实际提供行情的标的；缺失行情不会被伪造或前向填充。
#[derive(Debug, Clone)]
pub struct MultiAssetData {
    calendar: Vec<String>,
    bars_by_date: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Bar>>,
    symbols: Vec<String>,
}

impl MultiAssetData {
    pub fn new(mut bars: Vec<Bar>) -> Result<Self, String> {
        use std::collections::{BTreeMap, BTreeSet};

        if bars.is_empty() {
            return Err("多标的行情不能为空".into());
        }

        let mut by_symbol: BTreeMap<String, Vec<Bar>> = BTreeMap::new();
        for bar in bars.drain(..) {
            if bar.symbol.trim().is_empty() {
                return Err(format!("交易日期 {} 的 symbol 不能为空", bar.date));
            }
            by_symbol.entry(bar.symbol.clone()).or_default().push(bar);
        }

        for (symbol, symbol_bars) in &by_symbol {
            validate_bars(symbol_bars).map_err(|e| format!("{symbol}: {e}"))?;
        }

        let mut calendar = BTreeSet::new();
        let mut bars_by_date: BTreeMap<String, BTreeMap<String, Bar>> = BTreeMap::new();
        for (symbol, mut symbol_bars) in by_symbol {
            symbol_bars.sort_by(|a, b| a.date.cmp(&b.date));
            for bar in symbol_bars {
                calendar.insert(bar.date.clone());
                bars_by_date
                    .entry(bar.date.clone())
                    .or_default()
                    .insert(symbol.clone(), bar);
            }
        }

        Ok(Self {
            calendar: calendar.into_iter().collect(),
            bars_by_date,
            symbols: bars_by_date
                .values()
                .flat_map(|day| day.keys().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        })
    }

    pub fn calendar(&self) -> &[String] {
        &self.calendar
    }

    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    pub fn bars_on(&self, date: &str) -> Option<&std::collections::BTreeMap<String, Bar>> {
        self.bars_by_date.get(date)
    }

    pub fn len(&self) -> usize {
        self.calendar.len()
    }

    pub fn is_empty(&self) -> bool {
        self.calendar.is_empty()
    }
}
