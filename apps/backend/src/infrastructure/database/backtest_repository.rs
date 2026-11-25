use sqlx::{PgPool, postgres::PgPoolOptions};

/// 建立 PostgreSQL 连接池。
///
/// 具体业务 SQL 由各业务模块自己的 Repository adapter 负责。
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(database_url)
        .await
}
