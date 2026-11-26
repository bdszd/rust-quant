use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use super::model::{NewUser, User, UserRole, UserStatus};

#[derive(Debug, Error)]
pub enum UserRepositoryError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("invalid user status in database: {0}")]
    InvalidStatus(String),

    #[error("invalid user role in database: {0}")]
    InvalidRole(String),
}

#[derive(Debug, FromRow)]
struct UserRow {
    id: Uuid,
    username: String,
    email: String,
    status: String,
    role: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<UserRow> for User {
    type Error = UserRepositoryError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        let status = match row.status.as_str() {
            "pending" => UserStatus::Pending,
            "active" => UserStatus::Active,
            "disabled" => UserStatus::Disabled,
            other => return Err(UserRepositoryError::InvalidStatus(other.to_owned())),
        };

        let role = match row.role.as_str() {
            "customer" => UserRole::Customer,
            "admin" => UserRole::Admin,
            other => return Err(UserRepositoryError::InvalidRole(other.to_owned())),
        };

        Ok(Self {
            id: row.id,
            username: row.username,
            email: row.email,
            status,
            role,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Debug, Clone)]
pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 创建用户。role 使用数据库默认值 customer。
    pub async fn create(&self, new_user: &NewUser) -> Result<User, UserRepositoryError> {
        let id = Uuid::new_v4();

        let row = sqlx::query_as::<_, UserRow>(
            r#"
            INSERT INTO users (id, username, email)
            VALUES ($1, $2, $3)
            RETURNING
                id, username, email, status, role, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&new_user.username)
        .bind(&new_user.email)
        .fetch_one(&self.pool)
        .await?;

        User::try_from(row)
    }

    /// 在外部事务中创建用户，由调用方控制事务生命周期。
    pub async fn create_in_transaction(
        tx: &mut Transaction<'_, Postgres>,
        new_user: &NewUser,
    ) -> Result<User, UserRepositoryError> {
        let id = Uuid::new_v4();

        let row = sqlx::query_as::<_, UserRow>(
            r#"
            INSERT INTO users (id, username, email)
            VALUES ($1, $2, $3)
            RETURNING
                id, username, email, status, role, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&new_user.username)
        .bind(&new_user.email)
        .fetch_one(&mut **tx)
        .await?;

        User::try_from(row)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, UserRepositoryError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, username, email, status, role, created_at, updated_at
            FROM users
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(User::try_from).transpose()
    }

    pub async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<User>, UserRepositoryError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, username, email, status, role, created_at, updated_at
            FROM users
            WHERE username = $1
            "#,
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        row.map(User::try_from).transpose()
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<User>, UserRepositoryError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, username, email, status, role, created_at, updated_at
            FROM users
            WHERE email = $1
            "#,
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        row.map(User::try_from).transpose()
    }

    pub async fn update_status(
        &self,
        id: Uuid,
        status: UserStatus,
    ) -> Result<Option<User>, UserRepositoryError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            UPDATE users
            SET status = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, username, email, status, role, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(status.as_str())
        .fetch_optional(&self.pool)
        .await?;

        row.map(User::try_from).transpose()
    }

    /// 修改用户角色。该方法只提供给受控的管理流程调用。
    pub async fn update_role(
        &self,
        id: Uuid,
        role: UserRole,
    ) -> Result<Option<User>, UserRepositoryError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            UPDATE users
            SET role = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, username, email, status, role, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(role.as_str())
        .fetch_optional(&self.pool)
        .await?;

        row.map(User::try_from).transpose()
    }
}
