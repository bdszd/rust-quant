use std::sync::Arc;

use async_trait::async_trait;
use market_data::MarketDataRepository;

use crate::modules::dataset::application::{ports::DatasetCatalog, query::DatasetSummary};

/// 基于 market-data Repository 的 PostgreSQL 数据集目录。
///
/// SQL 由 market-data crate 负责；Backend 只把数据库模型转换为 API 查询模型。
pub struct PgDatasetCatalog {
    repository: Arc<dyn MarketDataRepository>,
}

impl PgDatasetCatalog {
    pub fn new(repository: Arc<dyn MarketDataRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl DatasetCatalog for PgDatasetCatalog {
    async fn list(&self) -> Result<Vec<DatasetSummary>, String> {
        self.repository
            .list_datasets()
            .await
            .map_err(|error| format!("查询 PostgreSQL 数据集列表失败：{error}"))
            .map(|datasets| {
                datasets
                    .into_iter()
                    .map(|dataset| DatasetSummary {
                        id: dataset.id,
                        name: dataset.name,
                        rows: dataset.rows,
                    })
                    .collect()
            })
    }
}
