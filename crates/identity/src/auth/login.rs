use thiserror::Error;

use crate::{
    auth::{PasswordError, verify_password},
    credential::{CredentialRepository, CredentialRepositoryError},
    user::{User, UserRepository, UserRepositoryError, UserStatus},
};

/// 登录服务错误。
#[derive(Debug, Error)]
pub enum LoginError {
    /// 账号不存在或密码错误。
    ///
    /// 对外统一返回这个错误，避免泄露账号是否存在。
    #[error("invalid username or password")]
    InvalidCredentials,

    /// 用户账号当前不可用。
    #[error("user account is not active")]
    AccountNotActive,

    #[error("password verification failed: {0}")]
    Password(#[from] PasswordError),

    #[error("user repository error: {0}")]
    UserRepository(#[from] UserRepositoryError),

    #[error("credential repository error: {0}")]
    CredentialRepository(#[from] CredentialRepositoryError),
}

/// 登录服务。
///
/// 负责查找用户、检查账号状态以及验证密码。
#[derive(Debug, Clone)]
pub struct LoginService {
    user_repository: UserRepository,
    credential_repository: CredentialRepository,
}

impl LoginService {
    /// 创建登录服务。
    pub fn new(
        user_repository: UserRepository,
        credential_repository: CredentialRepository,
    ) -> Self {
        Self {
            user_repository,
            credential_repository,
        }
    }

    /// 使用用户名或邮箱登录。
    ///
    /// identifier 可以是用户名，也可以是邮箱。
    /// 密码验证成功后返回用户基本信息。
    pub async fn login(&self, identifier: &str, password: &str) -> Result<User, LoginError> {
        // 1. 清理账号输入。
        let identifier = identifier.trim();

        if identifier.is_empty() || password.is_empty() {
            return Err(LoginError::InvalidCredentials);
        }

        // 2. 优先按用户名查找。
        let user = match self.user_repository.find_by_username(identifier).await? {
            Some(user) => Some(user),
            None => {
                // 3. 如果用户名不存在，再按邮箱查找。
                let email = identifier.to_lowercase();

                self.user_repository.find_by_email(&email).await?
            }
        };

        // 4. 用户不存在时，返回统一认证错误。
        let user = user.ok_or(LoginError::InvalidCredentials)?;

        // 5. 只有 active 状态的用户允许登录。
        if user.status != UserStatus::Active {
            return Err(LoginError::AccountNotActive);
        }

        // 6. 根据用户 ID 查询密码凭据。
        let credential = self
            .credential_repository
            .find_by_user_id(user.id)
            .await?
            .ok_or(LoginError::InvalidCredentials)?;

        // 7. 使用 Argon2 验证用户输入的密码。
        let password_matches = verify_password(password, &credential.password_hash)?;

        if !password_matches {
            return Err(LoginError::InvalidCredentials);
        }

        // 8. 验证通过，返回用户信息。
        Ok(user)
    }
}
