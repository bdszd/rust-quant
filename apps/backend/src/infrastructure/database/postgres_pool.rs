use std::{env, time::Duration};

use sqlx::{PgPool, postgres::PgPoolOptions};

/// 建立 PostgreSQL 连接池。
///
/// 每个 Backend 实例都有独立连接池，因此多实例部署时，
/// 所有实例的 max_connections 总和必须小于数据库可用连接数。
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let max_connections = env::var("RUST_QUANT_DB_MAX_CONNECTIONS")
        .ok()
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|error| sqlx::Error::Configuration(
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("RUST_QUANT_DB_MAX_CONNECTIONS 必须是正整数：{error}")).into()
        ))?
        .unwrap_or(10);

    if max_connections == 0 {
        return Err(sqlx::Error::Configuration(
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "RUST_QUANT_DB_MAX_CONNECTIONS 必须大于 0").into()
        ));
    }

    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
}
