use std::path::PathBuf;

use crate::modules::dataset::application::ports::DatasetExistence;

/// 旧版 CSV 数据集存在性检查。
///
/// 仅供当前尚未切换到 PostgreSQL 行情读取的回测链路使用。
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

        if parts.next().is_some() || !valid_component(symbol) || !valid_component(date_range) {
            return Err("dataset 必须使用有效的 股票代码/日期范围 格式".to_string());
        }

        Ok(self.root.join(symbol).join(date_range))
    }
}

impl DatasetExistence for FilesystemDatasetCatalog {
    fn exists(&self, dataset: &str) -> Result<bool, String> {
        let directory = self.dataset_dir(dataset)?;
        Ok(directory.join("bars.csv").is_file()
            && directory.join("quality_report.json").is_file())
    }
}
