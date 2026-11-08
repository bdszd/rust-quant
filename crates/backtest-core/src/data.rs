use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

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

/// 数据加载阶段的错误。
///
/// 这里只描述文件读取、CSV 反序列化和多标的合并冲突，
/// 不负责判断行情数据是否符合金融业务规则。
#[derive(Debug)]
pub enum DataLoadError {
    Csv {
        path: String,
        source: csv::Error,
    },
    DuplicateBar {
        path: String,
        symbol: String,
        date: String,
    },
}

impl fmt::Display for DataLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Csv { path, source } => {
                write!(f, "加载行情文件失败（{path}）：{source}")
            }
            Self::DuplicateBar { path, symbol, date } => {
                write!(
                    f,
                    "合并行情文件失败（{path}）：标的 {symbol} 在 {date} 存在重复 Bar"
                )
            }
        }
    }
}

impl Error for DataLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Csv { source, .. } => Some(source),
            Self::DuplicateBar { .. } => None,
        }
    }
}

/// 加载单个 CSV 数据文件。
///
/// 只负责 CSV 读取、Bar 反序列化和按日期排序；
/// 数据格式转换、数据清洗和行情有效性校验由上游数据模块负责。
pub fn load_data(path: &str) -> Result<Vec<Bar>, DataLoadError> {
    let mut reader = csv::Reader::from_path(path).map_err(|source| DataLoadError::Csv {
        path: path.to_string(),
        source,
    })?;

    let mut bars = Vec::new();

    for row in reader.deserialize() {
        let bar: Bar = row.map_err(|source| DataLoadError::Csv {
            path: path.to_string(),
            source,
        })?;
        bars.push(bar);
    }

    bars.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(bars)
}

/// 多标的行情集合。
///
/// calendar 是所有 Bar 日期的并集；
/// bars_by_date 只保存实际加载到的行情，不补造缺失 Bar。
#[derive(Debug, Clone)]
pub struct MultiAssetData {
    calendar: Vec<String>,
    bars_by_date: BTreeMap<String, BTreeMap<String, Bar>>,
    symbols: Vec<String>,
}

impl MultiAssetData {
    pub fn calendar(&self) -> &[String] {
        &self.calendar
    }

    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    pub fn bars_on(&self, date: &str) -> Option<&BTreeMap<String, Bar>> {
        self.bars_by_date.get(date)
    }

    pub fn len(&self) -> usize {
        self.calendar.len()
    }

    pub fn is_empty(&self) -> bool {
        self.calendar.is_empty()
    }
}

/// 加载并整合多个行情文件。
///
/// 交易日历取所有 Bar 日期的并集，标的集合取所有 Bar 的 symbol。
/// 同一标的同一日期只能保存一条 Bar；发生键冲突时返回错误，避免静默覆盖。
/// 空路径列表或空文件会得到空数据，不视为加载错误。
pub fn load_multi_asset_data(paths: &[String]) -> Result<MultiAssetData, DataLoadError> {
    let mut calendar = BTreeSet::new();
    let mut symbols = BTreeSet::new();
    let mut bars_by_date: BTreeMap<String, BTreeMap<String, Bar>> = BTreeMap::new();

    for path in paths {
        let bars = load_data(path)?;

        for bar in bars {
            let date = bar.date.clone();
            let symbol = bar.symbol.clone();

            let bars_on_date = bars_by_date.entry(date.clone()).or_default();
            if bars_on_date.contains_key(&symbol) {
                return Err(DataLoadError::DuplicateBar {
                    path: path.clone(),
                    symbol,
                    date,
                });
            }

            calendar.insert(date);
            symbols.insert(symbol.clone());
            bars_on_date.insert(symbol, bar);
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
    use super::{DataLoadError, load_data, load_multi_asset_data};
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
            "rust_quant_data_loading_{}_{}.csv",
            std::process::id(),
            id
        ));
        fs::write(&path, contents)?;
        Ok(path)
    }

    fn path_string(path: &PathBuf) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn loads_single_file_and_sorts_bars_by_date() -> Result<(), Box<dyn Error>> {
        let path = write_test_csv(concat!(
            "symbol,date,open,high,low,close,volume\n",
            "AAA,2025-01-03,11,12,10,11,1200\n",
            "AAA,2025-01-02,10,11,9,10,1000\n",
        ))?;

        let result = load_data(&path_string(&path));
        let _ = fs::remove_file(&path);

        let bars = result?;
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].date, "2025-01-02");
        assert_eq!(bars[1].date, "2025-01-03");
        Ok(())
    }

    #[test]
    fn loads_empty_file_as_empty_bars() -> Result<(), Box<dyn Error>> {
        let path = write_test_csv("symbol,date,open,high,low,close,volume\n")?;

        let result = load_data(&path_string(&path));
        let _ = fs::remove_file(&path);

        assert!(result?.is_empty());
        Ok(())
    }

    #[test]
    fn returns_error_when_file_does_not_exist() {
        let path = std::env::temp_dir()
            .join(format!("rust_quant_missing_{}.csv", std::process::id()));

        let result = load_data(&path.to_string_lossy());
        assert!(matches!(result, Err(DataLoadError::Csv { .. })));
    }

    #[test]
    fn returns_error_when_csv_cannot_be_deserialized() -> Result<(), Box<dyn Error>> {
        let path = write_test_csv(concat!(
            "symbol,date,open,high,low,close,volume\n",
            "AAA,2025-01-02,not-a-number,11,9,10,1000\n",
        ))?;

        let result = load_data(&path_string(&path));
        let _ = fs::remove_file(&path);

        assert!(matches!(result, Err(DataLoadError::Csv { .. })));
        Ok(())
    }

    #[test]
    fn combines_files_into_union_calendar_without_filling_missing_bars()
    -> Result<(), Box<dyn Error>> {
        let aaa = write_test_csv(concat!(
            "symbol,date,open,high,low,close,volume\n",
            "AAA,2025-01-02,10,11,9,10,1000\n",
            "AAA,2025-01-03,11,12,10,11,1200\n",
        ))?;
        let bbb = write_test_csv(concat!(
            "symbol,date,open,high,low,close,volume\n",
            "BBB,2025-01-02,20,21,19,20,2000\n",
            "BBB,2025-01-06,21,22,20,21,2200\n",
        ))?;

        let paths = vec![path_string(&aaa), path_string(&bbb)];
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
        assert_eq!(data.len(), 3);
        assert_eq!(data.bars_on("2025-01-02").unwrap().len(), 2);
        assert_eq!(data.bars_on("2025-01-03").unwrap().len(), 1);
        assert!(!data.bars_on("2025-01-03").unwrap().contains_key("BBB"));
        Ok(())
    }

    #[test]
    fn empty_files_and_empty_path_list_produce_empty_dataset()
    -> Result<(), Box<dyn Error>> {
        let path = write_test_csv("symbol,date,open,high,low,close,volume\n")?;

        let data = load_multi_asset_data(&[path_string(&path)])?;
        let empty = load_multi_asset_data(&[])?;

        let _ = fs::remove_file(&path);

        assert!(data.is_empty());
        assert!(data.symbols().is_empty());
        assert!(empty.is_empty());
        Ok(())
    }

    #[test]
    fn duplicate_symbol_and_date_returns_merge_conflict()
    -> Result<(), Box<dyn Error>> {
        let first = write_test_csv(concat!(
            "symbol,date,open,high,low,close,volume\n",
            "AAA,2025-01-02,10,11,9,10,1000\n",
        ))?;
        let second = write_test_csv(concat!(
            "symbol,date,open,high,low,close,volume\n",
            "AAA,2025-01-02,12,13,11,12,1500\n",
        ))?;

        let paths = vec![path_string(&first), path_string(&second)];
        let result = load_multi_asset_data(&paths);

        let _ = fs::remove_file(&first);
        let _ = fs::remove_file(&second);

        assert!(matches!(result, Err(DataLoadError::DuplicateBar { .. })));
        Ok(())
    }
}
