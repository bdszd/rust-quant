use crate::{
    domain::{CreateDatasetRequest, DailyBar, IngestionRequest, IngestionResult, QueryRequest},
    normalize::{normalize_daily_bar, normalize_date},
    source::{MarketDataSource, SourceError},
    storage::{DatasetInfo, MarketDataRepository, StorageError},
    validation::validate_daily_bar,
};
use std::{collections::HashSet, sync::Arc};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MarketDataError {
    #[error("请求参数错误：{0}")]
    InvalidRequest(String),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

pub struct MarketDataService {
    source: Arc<dyn MarketDataSource>,
    repository: Arc<dyn MarketDataRepository>,
}

impl MarketDataService {
    pub fn new(
        source: Arc<dyn MarketDataSource>,
        repository: Arc<dyn MarketDataRepository>,
    ) -> Self {
        Self { source, repository }
    }

    pub async fn list_datasets(&self) -> Result<Vec<DatasetInfo>, MarketDataError> {
        Ok(self.repository.list_datasets().await?)
    }

    pub async fn dataset_exists(&self, dataset_id: &str) -> Result<bool, MarketDataError> {
        Ok(self.repository.dataset_exists(dataset_id).await?)
    }

    pub async fn create_dataset(
        &self,
        request: &CreateDatasetRequest,
    ) -> Result<(), MarketDataError> {
        if request.name.trim().is_empty() || request.name.len() > 120 {
            return Err(MarketDataError::InvalidRequest(
                "数据集名称不能为空且不能超过 120 字节".into(),
            ));
        }
        self.repository.create_dataset(request).await?;
        Ok(())
    }

    pub async fn ingest(
        &self,
        request: IngestionRequest,
    ) -> Result<IngestionResult, MarketDataError> {
        let (start, end) =
            Self::normalize_and_validate_range(&request.start_date, &request.end_date)?;
        if request.symbols.is_empty() {
            return Err(MarketDataError::InvalidRequest("symbols 不能为空".into()));
        }

        // 数据源只接收标准化后的日期，避免不同数据源各自处理日期格式。
        let mut source_request = request.clone();
        source_request.start_date = start.clone();
        source_request.end_date = end.clone();
        let raw = self.source.fetch_daily_bars(&source_request).await?;

        let mut result = IngestionResult {
            received: raw.len(),
            ..Default::default()
        };

        // Persist the complete source snapshot first. This commits independently,
        // so later normalization/validation failures cannot erase the raw records.
        result.raw_stored = self
            .repository
            .store_raw_daily_bars(&request.dataset_id, &raw)
            .await?;

        let requested: HashSet<String> = request
            .symbols
            .iter()
            .map(|symbol| symbol.as_str().to_owned())
            .collect();

        let mut seen = HashSet::new();
        let mut accepted: Vec<DailyBar> = Vec::new();

        for row in raw {
            let bar = match normalize_daily_bar(row) {
                Ok(bar) => bar,
                Err(_) => {
                    result.rejected += 1;
                    continue;
                }
            };

            let key = format!("{}:{}", bar.symbol, bar.trade_date);
            if !requested.contains(bar.symbol.as_str())
                || bar.trade_date < start
                || bar.trade_date > end
                || !seen.insert(key)
                || validate_daily_bar(&bar).is_err()
            {
                result.rejected += 1;
                continue;
            }

            accepted.push(bar);
        }

        accepted.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then(a.trade_date.cmp(&b.trade_date))
        });

        result.accepted = accepted.len();
        result.upserted = self
            .repository
            .upsert_daily_bars(request.dataset_id, &accepted)
            .await?;

        Ok(result)
    }

    pub async fn query_daily_bars(
        &self,
        request: QueryRequest,
    ) -> Result<Vec<DailyBar>, MarketDataError> {
        let (start, end) =
            Self::normalize_and_validate_range(&request.start_date, &request.end_date)?;
        if request.symbols.is_empty() {
            return Err(MarketDataError::InvalidRequest("symbols 不能为空".into()));
        }

        // Repository 接收统一格式的日期，不依赖调用方传入的格式。
        let mut normalized_request = request;
        normalized_request.start_date = start;
        normalized_request.end_date = end;

        let bars = self
            .repository
            .query_daily_bars(&normalized_request)
            .await?;
        Ok(bars
            .into_iter()
            .filter(|bar| validate_daily_bar(bar).is_ok())
            .collect())
    }

    fn normalize_and_validate_range(
        start: &str,
        end: &str,
    ) -> Result<(String, String), MarketDataError> {
        let start = normalize_date(start).map_err(MarketDataError::InvalidRequest)?;
        let end = normalize_date(end).map_err(MarketDataError::InvalidRequest)?;

        if start > end {
            return Err(MarketDataError::InvalidRequest(
                "start_date 不能晚于 end_date".into(),
            ));
        }

        Ok((start, end))
    }
}
