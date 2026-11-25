pub mod domain;
pub mod normalize;
pub mod service;
pub mod source;
pub mod storage;
pub mod validation;

pub use domain::{DailyBar, IngestionRequest, IngestionResult, QueryRequest, RawDailyBar, Symbol};
pub use service::{MarketDataError, MarketDataService};
pub use source::{CsvSource, MarketDataSource, TushareSource};
pub use storage::{MarketDataRepository, PostgresRepository};
