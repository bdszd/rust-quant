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
    pub(crate) fn from_bars(mut bars: Vec<Bar>) -> Result<Self, String> {
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

        let symbols = bars_by_date
            .values()
            .flat_map(|day| day.keys().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        Ok(Self {
            calendar: calendar.into_iter().collect(),
            bars_by_date,
            symbols,
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

/// 从多个单标的 CSV 文件加载组合行情。每个文件必须只包含一个股票代码。
/// 各文件的交易日可以不同，最终交易日历取所有文件日期的并集。
pub fn load_multi_asset_data(paths: &[String]) -> Result<MultiAssetData, Box<dyn Error>> {
    use std::collections::{BTreeMap, BTreeSet};

    if paths.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "多标的行情文件路径不能为空",
        )
        .into());
    }

    let mut calendar = BTreeSet::new();
    let mut symbols = BTreeSet::new();
    let mut bars_by_date: BTreeMap<String, BTreeMap<String, Bar>> = BTreeMap::new();

    for path in paths {
        let bars = load_bars(path)?;
        let Some(first_bar) = bars.first() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("行情文件为空：{path}"),
            )
            .into());
        };

        let symbol = first_bar.symbol.trim();
        if symbol.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("行情文件缺少股票代码：{path}"),
            )
            .into());
        }

        if !symbols.insert(symbol.to_string()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("多个行情文件包含重复股票代码：{symbol}"),
            )
            .into());
        }

        for bar in bars {
            if bar.symbol.trim().is_empty() || bar.symbol != symbol {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("行情文件 {path} 必须只包含股票代码 {symbol}"),
                )
                .into());
            }

            calendar.insert(bar.date.clone());
            bars_by_date
                .entry(bar.date.clone())
                .or_default()
                .insert(symbol.to_string(), bar);
        }
    }

    Ok(MultiAssetData {
        calendar: calendar.into_iter().collect(),
        bars_by_date,
        symbols: symbols.into_iter().collect(),
    })
}


#[cfg(test)]
mod tests {
    use super::load_multi_asset_data;
    use std::{
        error::Error,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_FILE_ID: AtomicUsize = AtomicUsize::new(0);

    fn write_test_csv(contents: &str) -> Result<PathBuf, Box<dyn Error>> {
        let id = NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rust_quant_multi_asset_{}_{}.csv",
            std::process::id(),
            id
        ));
        fs::write(&path, contents)?;
        Ok(path)
    }

    #[test]
    fn loads_multiple_single_symbol_files_and_builds_union_calendar(
    ) -> Result<(), Box<dyn Error>> {
        let aaa = write_test_csv(concat!(
            "symbol,date,open,high,low,close,volume\n",
            "AAA,2025-01-02,10,10.5,9.5,10,1000\n",
            "AAA,2025-01-03,11,11.5,10.5,11,1200\n",
        ))?;
        let bbb = write_test_csv(concat!(
            "symbol,date,open,high,low,close,volume\n",
            "BBB,2025-01-02,20,20.5,19.5,20,2000\n",
            "BBB,2025-01-06,21,21.5,20.5,21,2200\n",
        ))?;

        let paths = vec![
            aaa.to_string_lossy().into_owned(),
            bbb.to_string_lossy().into_owned(),
        ];
        let result = load_multi_asset_data(&paths);

        let _ = fs::remove_file(&aaa);
        let _ = fs::remove_file(&bbb);

        let data = result?;
        assert_eq!(
            data.calendar(),
            &vec![
                "2025-01-02".to_string(),
                "2025-01-03".to_string(),
                "2025-01-06".to_string(),
            ]
        );
        assert_eq!(data.symbols(), &vec!["AAA".to_string(), "BBB".to_string()]);
        assert_eq!(data.bars_on("2025-01-03").unwrap().len(), 1);
        assert!(!data.bars_on("2025-01-03").unwrap().contains_key("BBB"));
        Ok(())
    }
}
