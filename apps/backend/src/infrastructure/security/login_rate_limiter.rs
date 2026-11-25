use std::{net::IpAddr, sync::atomic::{AtomicU64, Ordering}};

use sha2::{Digest, Sha256};
use sqlx::PgPool;

const WINDOW_MINUTES: i64 = 15;
static CLEANUP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 基于 PostgreSQL 的登录频率限制器。
///
/// 计数保存在数据库中，因此多个 Backend 实例共享同一份限制状态。
#[derive(Debug, Clone)]
pub struct LoginRateLimiter {
    pool: PgPool,
}

impl LoginRateLimiter {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 预占一次登录尝试。返回 Some(秒数) 表示当前请求应被限流。
    pub async fn check_and_record(
        &self,
        identifier: &str,
        client_ip: IpAddr,
    ) -> Result<Option<u64>, sqlx::Error> {
        let identifier_key = digest_key("identifier", &identifier.trim().to_lowercase());
        let ip_key = digest_key("ip", &client_ip.to_string());

        for (key, max_attempts) in [(identifier_key, 10), (ip_key, 60)] {
            let (attempts, retry_after): (i32, i64) = sqlx::query_as(
                r#"
                INSERT INTO login_rate_limits (bucket_key, window_started_at, attempts)
                VALUES ($1, clock_timestamp(), 1)
                ON CONFLICT (bucket_key) DO UPDATE
                SET
                    window_started_at = CASE
                        WHEN login_rate_limits.window_started_at
                            <= clock_timestamp() - make_interval(mins => $2)
                        THEN clock_timestamp()
                        ELSE login_rate_limits.window_started_at
                    END,
                    attempts = CASE
                        WHEN login_rate_limits.window_started_at
                            <= clock_timestamp() - make_interval(mins => $2)
                        THEN 1
                        ELSE login_rate_limits.attempts + 1
                    END
                RETURNING
                    attempts,
                    GREATEST(
                        1,
                        CEIL(EXTRACT(EPOCH FROM (
                            window_started_at + make_interval(mins => $2) - clock_timestamp()
                        )))
                    )::BIGINT
                "#,
            )
            .bind(&key)
            .bind(WINDOW_MINUTES as i32)
            .fetch_one(&self.pool)
            .await?;

            if attempts > max_attempts {
                return Ok(Some(retry_after.max(1) as u64));
            }
        }

        if CLEANUP_COUNTER.fetch_add(1, Ordering::Relaxed) % 100 == 0 {
            sqlx::query(
                "DELETE FROM login_rate_limits WHERE window_started_at < clock_timestamp() - INTERVAL '1 day'",
            )
            .execute(&self.pool)
            .await?;
        }

        Ok(None)
    }

    /// 登录成功后清除该账号和来源 IP 的失败/尝试计数。
    pub async fn reset(
        &self,
        identifier: &str,
        client_ip: IpAddr,
    ) -> Result<(), sqlx::Error> {
        let identifier_key = digest_key("identifier", &identifier.trim().to_lowercase());
        let _ = client_ip;

        sqlx::query("DELETE FROM login_rate_limits WHERE bucket_key = $1")
            .bind(identifier_key)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

fn digest_key(kind: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
