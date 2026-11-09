use crate::domain::{AmountUnit, IngestionRequest, PriceUnit, RawDailyBar, VolumeUnit};
use async_trait::async_trait;
use reqwest::Client;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::{path::PathBuf, str::FromStr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("数据源配置错误：{0}")]
    Configuration(String),
    #[error("数据源请求失败：{0}")]
    Request(String),
    #[error("数据源响应格式错误：{0}")]
    InvalidResponse(String),
    #[error("CSV 错误：{0}")]
    Csv(#[from] csv::Error),
    #[error("文件错误：{0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait MarketDataSource: Send + Sync {
    async fn fetch_daily_bars(&self, request: &IngestionRequest) -> Result<Vec<RawDailyBar>, SourceError>;
}

#[derive(Debug, Clone)]
pub struct CsvSource { path: PathBuf }
impl CsvSource {
    pub fn new(path: impl Into<PathBuf>) -> Self { Self { path: path.into() } }

    fn decimal(value: &str, field: &str) -> Result<Decimal, SourceError> {
        Decimal::from_str(value.trim()).map_err(|e| SourceError::InvalidResponse(
            format!("CSV 字段 {field} 数值无效：{value} ({e})")
        ))
    }

    fn read(&self) -> Result<Vec<RawDailyBar>, SourceError> {
        let mut reader = csv::Reader::from_path(&self.path)?;
        let headers = reader.headers()?.clone();
        let col = |name: &str| headers.iter().position(|v| v == name).ok_or_else(||
            SourceError::InvalidResponse(format!("CSV 缺少列：{name}"))
        );
        let (si, di, oi, hi, li, ci, vi) = (
            col("ts_code")?, col("trade_date")?, col("open")?, col("high")?,
            col("low")?, col("close")?, col("vol")?
        );
        let ai = headers.iter().position(|v| v == "amount");
        let mut output = Vec::new();
        for record in reader.records() {
            let row = record?;
            let get = |i: usize| row.get(i).ok_or_else(||
                SourceError::InvalidResponse("CSV 行字段不完整".to_owned())
            );
            let amount = ai.map(|i| get(i).and_then(|v| Self::decimal(v, "amount"))).transpose()?;
            output.push(RawDailyBar {
                symbol: get(si)?.to_owned(), trade_date: get(di)?.to_owned(),
                open: Self::decimal(get(oi)?, "open")?, high: Self::decimal(get(hi)?, "high")?,
                low: Self::decimal(get(li)?, "low")?, close: Self::decimal(get(ci)?, "close")?,
                volume: Self::decimal(get(vi)?, "vol")?, amount,
                price_unit: PriceUnit::Cny, volume_unit: VolumeUnit::Lots,
                amount_unit: amount.map(|_| AmountUnit::ThousandCny),
            });
        }
        Ok(output)
    }
}
#[async_trait]
impl MarketDataSource for CsvSource {
    async fn fetch_daily_bars(&self, _request: &IngestionRequest) -> Result<Vec<RawDailyBar>, SourceError> {
        self.read()
    }
}

#[derive(Debug, Clone)]
pub struct TushareSource { client: Client, token: String, endpoint: String }
impl TushareSource {
    pub fn new(token: impl Into<String>) -> Result<Self, SourceError> {
        Self::with_endpoint(token, "https://api.tushare.pro")
    }
    pub fn with_endpoint(token: impl Into<String>, endpoint: impl Into<String>) -> Result<Self, SourceError> {
        let token = token.into();
        if token.trim().is_empty() {
            return Err(SourceError::Configuration("TUSHARE_TOKEN 不能为空".to_owned()));
        }
        Ok(Self {
            client: Client::builder().build().map_err(|e| SourceError::Configuration(e.to_string()))?,
            token, endpoint: endpoint.into(),
        })
    }
    fn api_date(value: &str) -> Result<String, SourceError> {
        let value = value.replace('-', "");
        if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(SourceError::Configuration(format!("日期格式错误：{value}")));
        }
        Ok(value)
    }
    fn decimal(value: &Value, field: &str) -> Result<Decimal, SourceError> {
        let text = match value {
            Value::String(v) => v.clone(),
            Value::Number(v) => v.to_string(),
            _ => return Err(SourceError::InvalidResponse(format!("Tushare 字段 {field} 无效"))),
        };
        Decimal::from_str(&text).map_err(|e| SourceError::InvalidResponse(format!("{field}: {e}")))
    }
    fn parse(value: Value) -> Result<Vec<RawDailyBar>, SourceError> {
        let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 0 {
            return Err(SourceError::Request(value.get("msg").and_then(Value::as_str)
                .unwrap_or("Tushare 返回错误").to_owned()));
        }
        let data = value.get("data").ok_or_else(|| SourceError::InvalidResponse("缺少 data".into()))?;
        let fields = data.get("fields").and_then(Value::as_array)
            .ok_or_else(|| SourceError::InvalidResponse("缺少 fields".into()))?;
        let items = data.get("items").and_then(Value::as_array)
            .ok_or_else(|| SourceError::InvalidResponse("缺少 items".into()))?;
        let names: Vec<&str> = fields.iter().filter_map(Value::as_str).collect();
        let idx = |name: &str| names.iter().position(|v| *v == name)
            .ok_or_else(|| SourceError::InvalidResponse(format!("缺少字段 {name}")));
        let (si, di, oi, hi, li, ci, vi, ai) = (
            idx("ts_code")?, idx("trade_date")?, idx("open")?, idx("high")?,
            idx("low")?, idx("close")?, idx("vol")?, idx("amount")?
        );
        let mut bars = Vec::new();
        for item in items {
            let row = item.as_array().ok_or_else(|| SourceError::InvalidResponse("行情行不是数组".into()))?;
            let get = |i: usize| row.get(i).ok_or_else(|| SourceError::InvalidResponse("行情行字段不完整".into()));
            let symbol = get(si)?.as_str().ok_or_else(|| SourceError::InvalidResponse("ts_code 无效".into()))?.to_owned();
            let trade_date = get(di)?.as_str().ok_or_else(|| SourceError::InvalidResponse("trade_date 无效".into()))?.to_owned();
            bars.push(RawDailyBar {
                symbol, trade_date,
                open: Self::decimal(get(oi)?, "open")?, high: Self::decimal(get(hi)?, "high")?,
                low: Self::decimal(get(li)?, "low")?, close: Self::decimal(get(ci)?, "close")?,
                volume: Self::decimal(get(vi)?, "vol")?,
                amount: Some(Self::decimal(get(ai)?, "amount")?),
                price_unit: PriceUnit::Cny, volume_unit: VolumeUnit::Lots,
                amount_unit: Some(AmountUnit::ThousandCny),
            });
        }
        Ok(bars)
    }
}
#[async_trait]
impl MarketDataSource for TushareSource {
    async fn fetch_daily_bars(&self, request: &IngestionRequest) -> Result<Vec<RawDailyBar>, SourceError> {
        let symbols = request.symbols.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
        let body = json!({
            "api_name": "daily", "token": self.token,
            "params": {
                "ts_code": symbols,
                "start_date": Self::api_date(&request.start_date)?,
                "end_date": Self::api_date(&request.end_date)?
            },
            "fields": "ts_code,trade_date,open,high,low,close,vol,amount"
        });
        let response = self.client.post(&self.endpoint).json(&body).send().await
            .map_err(|e| SourceError::Request(e.to_string()))?
            .error_for_status().map_err(|e| SourceError::Request(e.to_string()))?;
        let value = response.json::<Value>().await
            .map_err(|e| SourceError::InvalidResponse(e.to_string()))?;
        Self::parse(value)
    }
}
