use sqlx::PgPool;
use thiserror::Error;

use crate::{
    auth::{PasswordError, hash_password},
    credential::{CredentialRepository, CredentialRepositoryError, NewUserCredential},
    user::{NewUser, User, UserRepository, UserRepositoryError},
};

/// 用户注册过程中可能发生的错误。
#[derive(Debug, Error)]
pub enum RegistrationError {
    #[error("invalid registration input: {0}")]
    InvalidInput(&'static str),

    #[error("username already exists")]
    UsernameAlreadyExists,

    #[error("email already exists")]
    EmailAlreadyExists,

    #[error("password processing failed: {0}")]
    Password(#[from] PasswordError),

    #[error("user repository error: {0}")]
    UserRepository(#[from] UserRepositoryError),

    #[error("credential repository error: {0}")]
    CredentialRepository(#[from] CredentialRepositoryError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// 用户注册服务。
///
/// 负责协调用户信息、密码凭据和数据库事务。
#[derive(Debug, Clone)]
pub struct RegistrationService {
    pool: PgPool,
    users: UserRepository,
}

impl RegistrationService {
    /// 创建注册服务。
    pub fn new(pool: PgPool) -> Self {
        Self {
            users: UserRepository::new(pool.clone()),
            pool,
        }
    }

    /// 注册一个新用户。
    pub async fn register(
        &self,
        new_user: &NewUser,
        password: &str,
    ) -> Result<User, RegistrationError> {
        // 1. 规范化输入。
        let username = new_user.username.trim().to_owned();
        let email = new_user.email.trim().to_ascii_lowercase();

        // 2. 验证注册信息。
        Self::validate_input(&username, &email, password)?;

        let normalized_user = NewUser { username, email };

        // 3. 检查用户名是否已经存在。
        if self
            .users
            .find_by_username(&normalized_user.username)
            .await?
            .is_some()
        {
            return Err(RegistrationError::UsernameAlreadyExists);
        }

        // 4. 检查邮箱是否已经存在。
        if self
            .users
            .find_by_email(&normalized_user.email)
            .await?
            .is_some()
        {
            return Err(RegistrationError::EmailAlreadyExists);
        }

        // 5. 对密码进行 Argon2 哈希。
        let password_hash = hash_password(password)?;

        // 6. 开启数据库事务。
        let mut tx = self.pool.begin().await?;

        // 7. 在事务中创建用户。
        let user = UserRepository::create_in_transaction(&mut tx, &normalized_user)
            .await
            .map_err(Self::map_user_error)?;

        // 8. 在同一个事务中创建密码凭据。
        let credential = NewUserCredential {
            user_id: user.id,
            password_hash,
        };

        CredentialRepository::create_in_transaction(&mut tx, &credential).await?;

        // 9. 提交事务。
        tx.commit().await?;

        // 10. 返回创建成功的用户。
        Ok(user)
    }

    /// 验证注册输入。
    fn validate_input(
        username: &str,
        email: &str,
        password: &str,
    ) -> Result<(), RegistrationError> {
        if username.is_empty() {
            return Err(RegistrationError::InvalidInput("username cannot be empty"));
        }

        if username.chars().count() > 50 {
            return Err(RegistrationError::InvalidInput(
                "username cannot exceed 50 characters",
            ));
        }

        if email.is_empty() || email.len() > 255 {
            return Err(RegistrationError::InvalidInput(
                "email must contain between 1 and 255 bytes",
            ));
        }

        if email.contains(char::is_whitespace) || !email.contains('@') {
            return Err(RegistrationError::InvalidInput("email format is invalid"));
        }

        if password.chars().count() < 8 {
            return Err(RegistrationError::InvalidInput(
                "password must contain at least 8 characters",
            ));
        }

        Ok(())
    }

    /// 将数据库唯一约束冲突转换成业务错误。
    fn map_user_error(error: UserRepositoryError) -> RegistrationError {
        if let UserRepositoryError::Database(sqlx::Error::Database(database_error)) = &error {
            match database_error.constraint() {
                Some("users_username_key") => {
                    return RegistrationError::UsernameAlreadyExists;
                }
                Some("users_email_key") => {
                    return RegistrationError::EmailAlreadyExists;
                }
                _ => {}
            }
        }

        RegistrationError::UserRepository(error)
    }
}
