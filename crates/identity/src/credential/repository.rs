use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use super::model::{NewUserCredential, UserCredential};

/// 凭据仓储错误。
#[derive(Debug, Error)]
pub enum CredentialRepositoryError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// PostgreSQL 查询结果。
#[derive(Debug, FromRow)]
struct CredentialRow {
    user_id: Uuid,
    password_hash: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<CredentialRow> for UserCredential {
    fn from(row: CredentialRow) -> Self {
        Self {
            user_id: row.user_id,
            password_hash: row.password_hash,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// 用户凭据仓储。
#[derive(Debug, Clone)]
pub struct CredentialRepository {
    pool: PgPool,
}

impl CredentialRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 在已有事务中创建用户凭据。
    ///
    /// 注意：事务由调用方创建和提交。
    /// 这样可以和 users 表的插入操作保持原子性。
    pub async fn create_in_transaction(
        tx: &mut Transaction<'_, Postgres>,
        new_credential: &NewUserCredential,
    ) -> Result<UserCredential, CredentialRepositoryError> {
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            INSERT INTO user_credentials (
                user_id,
                password_hash
            )
            VALUES ($1, $2)
            RETURNING
                user_id,
                password_hash,
                created_at,
                updated_at
            "#,
        )
        .bind(new_credential.user_id)
        .bind(&new_credential.password_hash)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row.into())
    }

    /// 根据用户 ID 查询密码凭据。
    pub async fn find_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserCredential>, CredentialRepositoryError> {
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            SELECT
                user_id,
                password_hash,
                created_at,
                updated_at
            FROM user_credentials
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Into::into))
    }

    /// 更新用户密码哈希。
    pub async fn update_password_hash(
        &self,
        user_id: Uuid,
        password_hash: &str,
    ) -> Result<Option<UserCredential>, CredentialRepositoryError> {
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            UPDATE user_credentials
            SET
                password_hash = $2,
                updated_at = NOW()
            WHERE user_id = $1
            RETURNING
                user_id,
                password_hash,
                created_at,
                updated_at
            "#,
        )
        .bind(user_id)
        .bind(password_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Into::into))
    }
}
