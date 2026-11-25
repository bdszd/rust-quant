use std::path::{Path, PathBuf};

use crate::modules::backtest::application::ports::DatasetCatalog;

/// 清洗后 CSV 数据集的文件系统目录。
///
/// 这里只负责定位数据集，不负责解析行情或运行回测。
pub struct FilesystemDatasetCatalog {
    root: PathBuf,
}

impl FilesystemDatasetCatalog {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn dataset_dir(&self, dataset: &str) -> Result<PathBuf, String> {
        let mut parts = dataset.split('/');
        let symbol = parts.next().unwrap_or_default();
        let date_range = parts.next().unwrap_or_default();

        let valid_component = |value: &str| {
            !value.is_empty()
                && value.trim() == value
                && value != "."
                && value != ".."
                && !value.contains('\\')
                && !value.contains(':')
        };

        if parts.next().is_some()
            || !valid_component(symbol)
            || !valid_component(date_range)
        {
            return Err("dataset 必须使用有效的 股票代码/日期范围 格式".to_string());
        }

        Ok(self.root.join(symbol).join(date_range))
    }
}

impl DatasetCatalog for FilesystemDatasetCatalog {
    fn exists(&self, dataset: &str) -> Result<bool, String> {
        let directory = self.dataset_dir(dataset)?;
        let bars = directory.join("bars.csv");
        let metadata = directory.join("quality_report.json");

        Ok(Path::new(&bars).is_file() && Path::new(&metadata).is_file())
    }
}
