use std::{path::PathBuf, fs};

use crate::{
    modules::{
        dataset::application::{ports::DatasetCatalog, query::DatasetSummary},
    },
};

use serde::Deserialize;

/// 清洗后 CSV 数据集的文件系统目录。
///
/// 这里只负责定位数据集，不负责解析行情或运行回测。
pub struct FilesystemDatasetCatalog {
    root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct DatasetMetadata {
    accepted_rows: usize,
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
        Ok(directory.join("bars.csv").is_file()
            && directory.join("quality_report.json").is_file())
    }

    fn list(&self) -> Result<Vec<DatasetSummary>, String> {
        let symbols = fs::read_dir(&self.root)
            .map_err(|error| format!("无法读取清洗数据目录：{error}"))?;
        let mut datasets = Vec::new();

        for symbol_entry in symbols {
            let symbol_entry = symbol_entry
                .map_err(|error| format!("读取股票代码目录失败：{error}"))?;
            let symbol_path = symbol_entry.path();
            if !symbol_path.is_dir() {
                continue;
            }

            let Some(symbol) = symbol_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            let date_ranges = fs::read_dir(&symbol_path)
                .map_err(|error| format!("读取 {symbol} 的日期目录失败：{error}"))?;
            for date_entry in date_ranges {
                let date_entry = date_entry
                    .map_err(|error| format!("读取日期目录失败：{error}"))?;
                let date_path = date_entry.path();
                if !date_path.is_dir() {
                    continue;
                }

                let Some(date_range) = date_path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };

                if !date_path.join("bars.csv").is_file() {
                    continue;
                }

                let metadata_path = date_path.join("quality_report.json");
                if !metadata_path.is_file() {
                    continue;
                }

                let metadata_content = fs::read_to_string(&metadata_path)
                    .map_err(|error| format!("读取数据集元数据失败：{metadata_path:?}: {error}"))?;
                let metadata: DatasetMetadata = serde_json::from_str(&metadata_content)
                    .map_err(|error| format!("解析数据集元数据失败：{metadata_path:?}: {error}"))?;

                datasets.push(DatasetSummary {
                    name: format!("{symbol}/{date_range}"),
                    rows: metadata.accepted_rows,
                });
            }
        }

        datasets.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(datasets)
    }
}
