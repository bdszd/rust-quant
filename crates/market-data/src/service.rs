use crate::{
    domain::{DailyBar, IngestionRequest, IngestionResult, QueryRequest},
    normalize::{normalize_daily_bar, normalize_date},
    source::{MarketDataSource, SourceError},
    storage::{MarketDataRepository, StorageError},
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
    pub fn new(source: Arc<dyn MarketDataSource>, repository: Arc<dyn MarketDataRepository>) -> Self {
        Self { source, repository }
    }

    pub async fn ingest(&self, request: IngestionRequest) -> Result<IngestionResult, MarketDataError> {
        Self::validate_range(&request.start_date, &request.end_date)?;
        if request.symbols.is_empty() {
            return Err(MarketDataError::InvalidRequest("symbols 不能为空".into()));
        }

        let raw = self.source.fetch_daily_bars(&request).await?;
        let mut result = IngestionResult { received: raw.len(), ..Default::default() };
        let requested: HashSet<String> = request.symbols.iter().map(|s| s.as_str().to_owned()).collect();
        let start = normalize_date(&request.start_date).map_err(MarketDataError::InvalidRequest)?;
        let end = normalize_date(&request.end_date).map_err(MarketDataError::InvalidRequest)?;
        let mut seen = HashSet::new();
        let mut accepted: Vec<DailyBar> = Vec::new();

        for row in raw {
            let bar = match normalize_daily_bar(row) {
                Ok(bar) => bar,
                Err(_) => { result.rejected += 1; continue; }
            };
            let key = format!("{}:{}", bar.symbol, bar.trade_date);
            if !requested.contains(bar.symbol.as_str()) || bar.trade_date < start
                || bar.trade_date > end || !seen.insert(key) || validate_daily_bar(&bar).is_err()
            {
                result.rejected += 1;
                continue;
            }
            accepted.push(bar);
        }

        accepted.sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.trade_date.cmp(&b.trade_date)));
        result.accepted = accepted.len();
        result.upserted = self.repository.upsert_daily_bars(&accepted).await?;
        Ok(result)
    }

    pub async fn query_daily_bars(&self, request: QueryRequest) -> Result<Vec<DailyBar>, MarketDataError> {
        Self::validate_range(&request.start_date, &request.end_date)?;
        if request.symbols.is_empty() {
            return Err(MarketDataError::InvalidRequest("symbols 不能为空".into()));
        }
        let bars = self.repository.query_daily_bars(&request).await?;
        Ok(bars.into_iter().filter(|bar| validate_daily_bar(bar).is_ok()).collect())
    }

    fn validate_range(start: &str, end: &str) -> Result<(), MarketDataError> {
        let start = normalize_date(start).map_err(MarketDataError::InvalidRequest)?;
        let end = normalize_date(end).map_err(MarketDataError::InvalidRequest)?;
        if start > end {
            return Err(MarketDataError::InvalidRequest("start_date 不能晚于 end_date".into()));
        }
        Ok(())
    }
}
