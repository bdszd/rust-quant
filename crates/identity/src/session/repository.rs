use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use thiserror::Error;
use uuid::Uuid;

use super::model::{NewSession, Session};

#[derive(Debug, Error)]
pub enum SessionRepositoryError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, FromRow)]
struct SessionRow {
    id: Uuid,
    user_id: Uuid,
    token_hash: String,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl From<SessionRow> for Session {
    fn from(row: SessionRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            token_hash: row.token_hash,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRepository {
    pool: PgPool,
}

impl SessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 创建一个 Session。
    pub async fn create(
        &self,
        new_session: &NewSession,
    ) -> Result<Session, SessionRepositoryError> {
        let id = Uuid::new_v4();

        let row = sqlx::query_as::<_, SessionRow>(
            r#"
            INSERT INTO sessions (
                id,
                user_id,
                token_hash,
                expires_at
            )
            VALUES ($1, $2, $3, $4)
            RETURNING
                id,
                user_id,
                token_hash,
                expires_at,
                revoked_at,
                created_at,
                last_used_at
            "#,
        )
        .bind(id)
        .bind(new_session.user_id)
        .bind(&new_session.token_hash)
        .bind(new_session.expires_at)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.into())
    }

    /// 根据 Token 哈希查找有效 Session。
    ///
    /// 同时更新 last_used_at。
    pub async fn find_active_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Session>, SessionRepositoryError> {
        let row = sqlx::query_as::<_, SessionRow>(
            r#"
            UPDATE sessions AS s
            SET last_used_at = NOW()
            FROM users AS u
            WHERE s.user_id = u.id
              AND s.token_hash = $1
              AND s.revoked_at IS NULL
              AND s.expires_at > NOW()
              AND u.status = 'active'
            RETURNING
                s.id,
                s.user_id,
                s.token_hash,
                s.expires_at,
                s.revoked_at,
                s.created_at,
                s.last_used_at
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Into::into))
    }

    /// 撤销指定 Session。
    pub async fn revoke(
        &self,
        session_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, SessionRepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE sessions
            SET revoked_at = NOW()
            WHERE id = $1
              AND user_id = $2
              AND revoked_at IS NULL
            "#,
        )
        .bind(session_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// 撤销用户的所有 Session。
    pub async fn revoke_all_for_user(&self, user_id: Uuid) -> Result<u64, SessionRepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE sessions
            SET revoked_at = NOW()
            WHERE user_id = $1
              AND revoked_at IS NULL
            "#,
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}
