use crate::domain::{DailyBar, QueryRequest, Symbol};
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("数据库操作失败：{0}")]
    Database(#[from] sqlx::Error),
    #[error("数据库行情数据格式错误：{0}")]
    InvalidData(String),
}

#[async_trait]
pub trait MarketDataRepository: Send + Sync {
    async fn upsert_daily_bars(&self, bars: &[DailyBar]) -> Result<usize, StorageError>;
    async fn query_daily_bars(&self, request: &QueryRequest) -> Result<Vec<DailyBar>, StorageError>;
}

#[derive(Clone)]
pub struct PostgresRepository { pool: PgPool }
impl PostgresRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
    pub async fn migrate(&self) -> Result<(), StorageError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
            .map_err(|e| StorageError::InvalidData(e.to_string()))
    }
}

#[async_trait]
impl MarketDataRepository for PostgresRepository {
    async fn upsert_daily_bars(&self, bars: &[DailyBar]) -> Result<usize, StorageError> {
        if bars.is_empty() { return Ok(0); }
        let mut tx = self.pool.begin().await?;
        for bar in bars {
            sqlx::query("INSERT INTO market_symbols(symbol) VALUES($1) ON CONFLICT DO NOTHING")
                .bind(bar.symbol.as_str()).execute(&mut *tx).await?;
            sqlx::query(r#"
                INSERT INTO market_daily_bars(symbol, trade_date, open, high, low, close, volume, amount)
                VALUES($1, $2::date, $3, $4, $5, $6, $7, $8)
                ON CONFLICT(symbol, trade_date) DO UPDATE SET
                    open=EXCLUDED.open, high=EXCLUDED.high, low=EXCLUDED.low,
                    close=EXCLUDED.close, volume=EXCLUDED.volume, amount=EXCLUDED.amount,
                    updated_at=NOW()
            "#)
            .bind(bar.symbol.as_str()).bind(&bar.trade_date)
            .bind(bar.open).bind(bar.high).bind(bar.low).bind(bar.close)
            .bind(bar.volume).bind(bar.amount).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(bars.len())
    }

    async fn query_daily_bars(&self, request: &QueryRequest) -> Result<Vec<DailyBar>, StorageError> {
        let symbols: Vec<String> = request.symbols.iter().map(|s| s.as_str().to_owned()).collect();
        let rows = sqlx::query(r#"
            SELECT symbol, trade_date::text AS trade_date, open, high, low, close, volume, amount
            FROM market_daily_bars
            WHERE symbol = ANY($1) AND trade_date BETWEEN $2::date AND $3::date
            ORDER BY symbol, trade_date
        "#)
        .bind(&symbols).bind(&request.start_date).bind(&request.end_date)
        .fetch_all(&self.pool).await?;

        rows.into_iter().map(|row| {
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
                trade_date, open, high, low, close, volume, amount,
            })
        }).collect()
    }
}
