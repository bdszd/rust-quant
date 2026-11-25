use std::sync::Arc;

use crate::modules::backtest::application::ports::DatasetCatalog;

pub mod query;

/// 数据集应用服务：负责组织数据集目录查询，不直接操作文件系统。
pub struct DatasetApplicationService {
    catalog: Arc<dyn DatasetCatalog>,
}

impl DatasetApplicationService {
    pub fn new(catalog: Arc<dyn DatasetCatalog>) -> Self {
        Self { catalog }
    }

    pub async fn list_datasets(&self) -> Result<Vec<query::DatasetSummary>, String> {
        let catalog = self.catalog.clone();
        tokio::task::spawn_blocking(move || catalog.list())
            .await
            .map_err(|error| format!("数据集扫描任务异常：{error}"))?
    }
}
