use std::{
    collections::HashMap,
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use rust_decimal::Decimal;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use uuid::Uuid;

const DATASET_ID: &str = "00000000-0000-0000-0000-000000000002";
const DATASET_NAME: &str = "000001.SZ 2006-2026 日线行情";
const BATCH_KEY: &str = "tushare-000001.SZ-20060101-20261106-v1";

const RAW_HEADERS: &[&str] = &[
    "ts_code", "trade_date", "open", "high", "low", "close",
    "pre_close", "change", "pct_chg", "vol", "amount",
];
const BAR_HEADERS: &[&str] = &[
    "symbol", "date", "open", "high", "low", "close", "volume", "amount",
];

#[derive(Debug)]
struct RawRow {
    symbol: String,
    trade_date: String,
    payload: Value,
}

#[derive(Debug)]
struct BarRow {
    symbol: String,
    trade_date: String,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: i64,
    amount: Decimal,
}

struct Snapshot {
    raw: Vec<RawRow>,
    bars: Vec<BarRow>,
    raw_sha256: String,
    bars_sha256: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let commit = match args.as_slice() {
        [] => false,
        [flag] if flag == "--check" => false,
        [flag] if flag == "--commit" => true,
        _ => return Err("用法：cargo run -p quant-backend --bin import_market_data -- [--check|--commit]".into()),
    };

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw_path = root.join("data/raw/tushare/000001.SZ/20060101_20261106.csv");
    let bars_path = root.join("data/cleaned/tushare/000001.SZ/20060101_20261106/bars.csv");

    let snapshot = load_and_validate(&raw_path, &bars_path)?;
    println!("本地文件核对通过：");
    println!("  Raw 行数：{}", snapshot.raw.len());
    println!("  标准行情行数：{}", snapshot.bars.len());
    println!("  日期范围：{} 至 {}", snapshot.bars.first().unwrap().trade_date, snapshot.bars.last().unwrap().trade_date);
    println!("  Raw SHA256：{}", snapshot.raw_sha256);
    println!("  Bars SHA256：{}", snapshot.bars_sha256);

    if !commit {
        println!("当前为检查模式，没有连接数据库，也没有写入任何数据。");
        println!("确认数据库 Migration 0006、0007、0008 已应用后，使用 --commit 才会导入。");
        return Ok(());
    }

    let database_url = env::var("DATABASE_URL")
        .map_err(|_| "导入模式必须设置 DATABASE_URL")?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(import(&database_url, snapshot))?;
    Ok(())
}

fn load_and_validate(raw_path: &Path, bars_path: &Path) -> Result<Snapshot, Box<dyn Error>> {
    let raw_bytes = fs::read(raw_path)?;
    let bars_bytes = fs::read(bars_path)?;
    let raw_sha256 = hex_sha256(&raw_bytes);
    let bars_sha256 = hex_sha256(&bars_bytes);

    let mut raw_reader = csv::Reader::from_reader(raw_bytes.as_slice());
    let raw_headers = raw_reader.headers()?.clone();
    ensure_headers(&raw_headers, RAW_HEADERS, "Raw")?;

    let mut raw = Vec::new();
    for (index, record) in raw_reader.records().enumerate() {
        let record = record?;
        let values = record.iter().collect::<Vec<_>>();
        if values.len() != RAW_HEADERS.len() {
            return Err(format!("Raw 第 {} 行列数不正确", index + 2).into());
        }
        let fields: HashMap<&str, &str> = RAW_HEADERS.iter().copied().zip(values.iter().copied()).collect();
        let symbol = required(fields["ts_code"], "ts_code", index)?;
        let trade_date = raw_date(fields["trade_date"], index)?;

        let mut payload = Map::new();
        for (key, value) in RAW_HEADERS.iter().zip(values.iter()) {
            payload.insert((*key).to_string(), Value::String((*value).to_string()));
        }
        raw.push(RawRow {
            symbol: symbol.to_string(),
            trade_date,
            payload: Value::Object(payload),
        });
    }

    let mut bars_reader = csv::Reader::from_reader(bars_bytes.as_slice());
    let bars_headers = bars_reader.headers()?.clone();
    ensure_headers(&bars_headers, BAR_HEADERS, "Cleaned")?;

    let mut bars = Vec::new();
    for (index, record) in bars_reader.records().enumerate() {
        let record = record?;
        let values = record.iter().collect::<Vec<_>>();
        if values.len() != BAR_HEADERS.len() {
            return Err(format!("Cleaned 第 {} 行列数不正确", index + 2).into());
        }
        let fields: HashMap<&str, &str> = BAR_HEADERS.iter().copied().zip(values.iter().copied()).collect();
        let symbol = required(fields["symbol"], "symbol", index)?.to_string();
        let trade_date = clean_date(fields["date"], index)?;
        let volume_decimal = decimal(fields["volume"], "volume", index)?;
        let rounded_volume = volume_decimal.round();
        if (volume_decimal - rounded_volume).abs() > Decimal::new(1, 6) {
            return Err(format!("Cleaned 第 {} 行 volume 不是整数股数：{}", index + 2, fields["volume"]).into());
        }
        let volume = rounded_volume.to_string().parse::<i64>()?;
        if volume < 0 {
            return Err(format!("Cleaned 第 {} 行 volume 为负数", index + 2).into());
        }

        bars.push(BarRow {
            symbol,
            trade_date,
            open: decimal(fields["open"], "open", index)?,
            high: decimal(fields["high"], "high", index)?,
            low: decimal(fields["low"], "low", index)?,
            close: decimal(fields["close"], "close", index)?,
            volume,
            amount: decimal(fields["amount"], "amount", index)?,
        });
    }

    if raw.is_empty() || raw.len() != bars.len() {
        return Err(format!("Raw/Cleaned 行数不一致或为空：Raw={}，Cleaned={}", raw.len(), bars.len()).into());
    }

    for (index, (raw_row, bar)) in raw.iter().zip(&bars).enumerate() {
        if raw_row.symbol != "000001.SZ" || bar.symbol != "000001.SZ" {
            return Err(format!("第 {} 条记录不是目标股票 000001.SZ", index + 1).into());
        }
        if raw_row.symbol != bar.symbol || raw_row.trade_date != bar.trade_date {
            return Err(format!("第 {} 条 Raw 与 Cleaned 的股票代码或交易日期不一致", index + 1).into());
        }
        if index > 0 && bars[index - 1].trade_date >= bar.trade_date {
            return Err(format!("Cleaned 日期未严格递增，位置：{}", index + 1).into());
        }

        let raw_open = decimal_from_payload(&raw_row.payload, "open", index)?;
        let raw_high = decimal_from_payload(&raw_row.payload, "high", index)?;
        let raw_low = decimal_from_payload(&raw_row.payload, "low", index)?;
        let raw_close = decimal_from_payload(&raw_row.payload, "close", index)?;
        let raw_volume = decimal_from_payload(&raw_row.payload, "vol", index)? * Decimal::from(100);
        let raw_amount = decimal_from_payload(&raw_row.payload, "amount", index)? * Decimal::from(1000);

        if raw_open != bar.open || raw_high != bar.high || raw_low != bar.low || raw_close != bar.close {
            return Err(format!("第 {} 条 Raw 与 Cleaned 的 OHLC 不一致", index + 1).into());
        }
        if (raw_volume - Decimal::from(bar.volume)).abs() > Decimal::new(1, 5) {
            return Err(format!("第 {} 条 Raw/Cleaned 成交量换算不一致", index + 1).into());
        }
        if (raw_amount - bar.amount).abs() > Decimal::new(1, 2) {
            return Err(format!("第 {} 条 Raw/Cleaned 成交额换算不一致", index + 1).into());
        }
    }

    Ok(Snapshot { raw, bars, raw_sha256, bars_sha256 })
}

fn ensure_headers(actual: &csv::StringRecord, expected: &[&str], label: &str) -> Result<(), Box<dyn Error>> {
    let actual: Vec<&str> = actual.iter().collect();
    if actual != expected {
        return Err(format!("{label} CSV 表头不符合预期：{actual:?}").into());
    }
    Ok(())
}

fn required<'a>(value: &'a str, field: &str, index: usize) -> Result<&'a str, Box<dyn Error>> {
    if value.trim().is_empty() {
        return Err(format!("第 {} 条记录字段 {field} 为空", index + 2).into());
    }
    Ok(value)
}

fn decimal(value: &str, field: &str, index: usize) -> Result<Decimal, Box<dyn Error>> {
    Decimal::from_str(value)
        .map_err(|_| format!("第 {} 行字段 {field} 不是有效数字：{value}", index + 2).into())
}

fn decimal_from_payload(payload: &Value, field: &str, index: usize) -> Result<Decimal, Box<dyn Error>> {
    let value = payload[field].as_str().ok_or("Raw payload 缺少数字字段")?;
    decimal(value, field, index)
}

fn raw_date(value: &str, index: usize) -> Result<String, Box<dyn Error>> {
    if value.len() != 8 || !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Raw 第 {} 行交易日期格式错误：{value}", index + 2).into());
    }
    Ok(format!("{}-{}-{}", &value[0..4], &value[4..6], &value[6..8]))
}

fn clean_date(value: &str, index: usize) -> Result<String, Box<dyn Error>> {
    if value.len() != 10 || value.as_bytes().get(4) != Some(&b'-') || value.as_bytes().get(7) != Some(&b'-') {
        return Err(format!("Cleaned 第 {} 行交易日期格式错误：{value}", index + 2).into());
    }
    Ok(value.to_string())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn import(database_url: &str, snapshot: Snapshot) -> Result<(), Box<dyn Error>> {
    let pool = PgPoolOptions::new().max_connections(2).connect(database_url).await?;
    import_transaction(&pool, snapshot).await?;
    pool.close().await;
    Ok(())
}

async fn import_transaction(pool: &PgPool, snapshot: Snapshot) -> Result<(), Box<dyn Error>> {
    let dataset_id = Uuid::parse_str(DATASET_ID)?;
    let mut tx = pool.begin().await?;

    // 同一个数据集导入批次串行执行，避免两个导入进程同时通过重复检查。
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(BATCH_KEY)
        .execute(&mut *tx)
        .await?;

    let existing_batch = sqlx::query(
        "SELECT raw_sha256, bars_sha256, raw_rows, standard_rows FROM market_data_import_batches WHERE batch_key = $1",
    )
    .bind(BATCH_KEY)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(row) = existing_batch {
        let raw_hash: String = row.try_get("raw_sha256")?;
        let bars_hash: String = row.try_get("bars_sha256")?;
        let raw_rows: i64 = row.try_get("raw_rows")?;
        let standard_rows: i64 = row.try_get("standard_rows")?;
        if raw_hash == snapshot.raw_sha256
            && bars_hash == snapshot.bars_sha256
            && raw_rows == snapshot.raw.len() as i64
            && standard_rows == snapshot.bars.len() as i64
        {
            tx.rollback().await?;
            println!("该文件快照已经导入过，指纹和行数一致；本次不重复写入。");
            return Ok(());
        }
        return Err("导入批次已存在，但文件指纹或行数不同。为避免覆盖/追加错误数据，已中止。".into());
    }

    let dataset_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM market_datasets WHERE id = $1)")
        .bind(dataset_id)
        .fetch_one(&mut *tx)
        .await?;
    if dataset_exists {
        return Err("目标 dataset_id 已存在，但没有对应导入批次记录。为避免重复追加 Raw 或混入旧数据，已中止；请先人工检查数据库。".into());
    }

    sqlx::query(
        r#"INSERT INTO market_datasets
           (id, name, owner_type, owner_user_id, source_type, status)
           VALUES ($1, $2, 'platform', NULL, 'tushare', 'active')"#,
    )
    .bind(dataset_id)
    .bind(DATASET_NAME)
    .execute(&mut *tx)
    .await?;

    sqlx::query("INSERT INTO market_symbols (symbol) VALUES ('000001.SZ') ON CONFLICT (symbol) DO NOTHING")
        .execute(&mut *tx)
        .await?;

    for row in &snapshot.raw {
        sqlx::query(
            r#"INSERT INTO market_raw_daily_bars
               (dataset_id, source_type, symbol, trade_date, raw_payload)
               VALUES ($1, 'tushare', $2, $3, $4)"#,
        )
        .bind(dataset_id)
        .bind(&row.symbol)
        .bind(&row.trade_date)
        .bind(&row.payload)
        .execute(&mut *tx)
        .await?;
    }

    for row in &snapshot.bars {
        sqlx::query(
            r#"INSERT INTO market_daily_bars
               (dataset_id, symbol, trade_date, open, high, low, close, volume, amount)
               VALUES ($1, $2, $3::date, $4::numeric, $5::numeric, $6::numeric,
                       $7::numeric, $8, $9::numeric)"#,
        )
        .bind(dataset_id)
        .bind(&row.symbol)
        .bind(&row.trade_date)
        .bind(row.open)
        .bind(row.high)
        .bind(row.low)
        .bind(row.close)
        .bind(row.volume)
        .bind(row.amount)
        .execute(&mut *tx)
        .await?;
    }

    let first_date = &snapshot.bars.first().ok_or("Cleaned 数据为空")?.trade_date;
    let last_date = &snapshot.bars.last().ok_or("Cleaned 数据为空")?.trade_date;

    sqlx::query(
        r#"INSERT INTO market_data_import_batches
           (batch_key, dataset_id, raw_sha256, bars_sha256, raw_rows,
            standard_rows, first_trade_date, last_trade_date)
           VALUES ($1, $2, $3, $4, $5, $6, $7::date, $8::date)"#,
    )
    .bind(BATCH_KEY)
    .bind(dataset_id)
    .bind(&snapshot.raw_sha256)
    .bind(&snapshot.bars_sha256)
    .bind(snapshot.raw.len() as i64)
    .bind(snapshot.bars.len() as i64)
    .bind(first_date)
    .bind(last_date)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    println!("导入事务已提交：dataset_id={DATASET_ID}");
    println!("Raw rows={}，standard rows={}", snapshot.raw.len(), snapshot.bars.len());
    Ok(())
}
