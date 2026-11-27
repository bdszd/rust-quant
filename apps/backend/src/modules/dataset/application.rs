use std::sync::Arc;

use self::ports::DatasetCatalog;

pub mod ports;
pub mod query;

/// 数据集应用服务：负责组织数据集目录查询，不直接操作数据库。
pub struct DatasetApplicationService {
    catalog: Arc<dyn DatasetCatalog>,
}

impl DatasetApplicationService {
    pub fn new(catalog: Arc<dyn DatasetCatalog>) -> Self {
        Self { catalog }
    }

    pub async fn list_datasets(&self) -> Result<Vec<query::DatasetSummary>, String> {
        self.catalog.list().await
    }
}
