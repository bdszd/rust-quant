use crate::domain::{
    CreateDatasetRequest, DailyBar, DatasetOwner, QueryRequest, RawDailyBar, Symbol,
};
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row, types::Json};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("数据库操作失败：{0}")]
    Database(#[from] sqlx::Error),
    #[error("数据库行情数据格式错误：{0}")]
    InvalidData(String),
}

pub struct DatasetInfo {
    pub id: String,
    pub name: String,
    pub rows: i64,
}

#[async_trait]
pub trait MarketDataRepository: Send + Sync {
    async fn create_dataset(&self, request: &CreateDatasetRequest) -> Result<(), StorageError>;

    async fn dataset_exists(&self, dataset_id: &str) -> Result<bool, StorageError>;

    async fn list_datasets(&self) -> Result<Vec<DatasetInfo>, StorageError>;

    /// Persist source snapshots independently before normalization begins.
    async fn store_raw_daily_bars(
        &self,
        dataset_id: &str,
        bars: &[RawDailyBar],
    ) -> Result<usize, StorageError>;

    async fn upsert_daily_bars(
        &self,
        dataset_id: String,
        bars: &[DailyBar],
    ) -> Result<usize, StorageError>;

    async fn query_daily_bars(&self, request: &QueryRequest)
    -> Result<Vec<DailyBar>, StorageError>;
}

#[derive(Clone)]
pub struct PostgresRepository {
    pool: PgPool,
}

impl PostgresRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MarketDataRepository for PostgresRepository {
    async fn dataset_exists(&self, dataset_id: &str) -> Result<bool, StorageError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM market_datasets WHERE id = $1::text::uuid AND status = 'active')",
        )
        .bind(dataset_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    async fn list_datasets(&self) -> Result<Vec<DatasetInfo>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT d.id::text AS id, d.name,
                   COUNT(b.trade_date)::bigint AS rows
            FROM market_datasets d
            LEFT JOIN market_daily_bars b ON b.dataset_id = d.id
            WHERE d.status = 'active'
              AND d.owner_type = 'platform'
            GROUP BY d.id, d.name, d.created_at
            ORDER BY d.created_at DESC, d.id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(DatasetInfo {
                    id: row.try_get("id")?,
                    name: row.try_get("name")?,
                    rows: row.try_get("rows")?,
                })
            })
            .collect()
    }

    async fn create_dataset(&self, request: &CreateDatasetRequest) -> Result<(), StorageError> {
        let (owner_type, owner_user_id) = match &request.owner {
            DatasetOwner::Platform => ("platform", None),
            DatasetOwner::User(user_id) => ("user", Some(user_id.as_str())),
        };

        sqlx::query(
            r#"
            INSERT INTO market_datasets
                (id, name, owner_type, owner_user_id, source_type, status)
            VALUES ($1::text::uuid, $2, $3, $4::text::uuid, $5, 'active')
            "#,
        )
        .bind(&request.id)
        .bind(&request.name)
        .bind(owner_type)
        .bind(owner_user_id)
        .bind(request.source.as_str())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn store_raw_daily_bars(
        &self,
        dataset_id: &str,
        bars: &[RawDailyBar],
    ) -> Result<usize, StorageError> {
        if bars.is_empty() {
            return Ok(0);
        }

        // This transaction is committed before service-level normalization starts.
        // Raw rows are append-only snapshots: repeated imports do not overwrite history.
        let mut tx = self.pool.begin().await?;
        for bar in bars {
            sqlx::query(
                r#"
                INSERT INTO market_raw_daily_bars
                    (dataset_id, source_type, symbol, trade_date, raw_payload)
                VALUES ($1::text::uuid, $2, $3, $4, $5)
                "#,
            )
            .bind(dataset_id)
            .bind(bar.source.as_str())
            .bind(&bar.symbol)
            .bind(&bar.trade_date)
            .bind(Json(&bar.raw_payload))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(bars.len())
    }

    async fn upsert_daily_bars(
        &self,
        dataset_id: String,
        bars: &[DailyBar],
    ) -> Result<usize, StorageError> {
        if bars.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await?;
        for bar in bars {
            sqlx::query("INSERT INTO market_symbols(symbol) VALUES($1) ON CONFLICT DO NOTHING")
                .bind(bar.symbol.as_str())
                .execute(&mut *tx)
                .await?;

            sqlx::query(
                r#"
                INSERT INTO market_daily_bars
                    (dataset_id, symbol, trade_date, open, high, low, close, volume, amount)
                VALUES ($1::text::uuid, $2, $3::date, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (dataset_id, symbol, trade_date) DO UPDATE SET
                    open = EXCLUDED.open,
                    high = EXCLUDED.high,
                    low = EXCLUDED.low,
                    close = EXCLUDED.close,
                    volume = EXCLUDED.volume,
                    amount = EXCLUDED.amount,
                    updated_at = NOW()
                "#,
            )
            .bind(&dataset_id)
            .bind(bar.symbol.as_str())
            .bind(&bar.trade_date)
            .bind(bar.open)
            .bind(bar.high)
            .bind(bar.low)
            .bind(bar.close)
            .bind(bar.volume)
            .bind(bar.amount)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(bars.len())
    }

    async fn query_daily_bars(
        &self,
        request: &QueryRequest,
    ) -> Result<Vec<DailyBar>, StorageError> {
        let symbols: Vec<String> = request
            .symbols
            .iter()
            .map(|symbol| symbol.as_str().to_owned())
            .collect();

        let rows = sqlx::query(
            r#"
            SELECT symbol, trade_date::text AS trade_date,
                   open, high, low, close, volume, amount
            FROM market_daily_bars
            WHERE dataset_id = $1::text::uuid
              AND symbol = ANY($2)
              AND trade_date BETWEEN $3::date AND $4::date
            ORDER BY symbol, trade_date
            "#,
        )
        .bind(&request.dataset_id)
        .bind(&symbols)
        .bind(&request.start_date)
        .bind(&request.end_date)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let symbol: String = row.try_get("symbol")?;
                let trade_date: String = row.try_get("trade_date")?;
                let open: Decimal = row.try_get("open")?;
                let high: Decimal = row.try_get("high")?;
                let low: Decimal = row.try_get("low")?;
                let close: Decimal = row.try_get("close")?;
                let volume: i64 = row.try_get("volume")?;
                let amount: Option<Decimal> = row.try_get("amount")?;

                Ok(DailyBar {
                    symbol: Symbol::new(symbol).map_err(StorageError::InvalidData)?,
                    trade_date,
                    open,
                    high,
                    low,
                    close,
                    volume,
                    amount,
                })
            })
            .collect()
    }
}
