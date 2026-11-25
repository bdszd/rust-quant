use sqlx::PgPool;
use thiserror::Error;

use crate::{
    credential::CredentialRepository,
    session::{IssuedSession, SessionError, SessionRepository, SessionService},
    user::UserRepository,
};

use super::{LoginError, LoginService};

/// 完整认证流程中的错误。
#[derive(Debug, Error)]
pub enum AuthenticationError {
    #[error("login failed: {0}")]
    Login(#[from] LoginError),

    #[error("session operation failed: {0}")]
    Session(#[from] SessionError),
}

/// 认证服务。
///
/// 负责协调登录验证和 Session 创建。
#[derive(Debug, Clone)]
pub struct AuthenticationService {
    login_service: LoginService,
    session_service: SessionService,
}

impl AuthenticationService {
    /// 根据数据库连接池创建认证服务。
    pub fn new(pool: PgPool) -> Self {
        let user_repository = UserRepository::new(pool.clone());

        let credential_repository = CredentialRepository::new(pool.clone());

        let session_repository = SessionRepository::new(pool);

        let login_service = LoginService::new(user_repository, credential_repository);

        let session_service = SessionService::new(session_repository);

        Self {
            login_service,
            session_service,
        }
    }

    /// 完整登录流程：
    ///
    /// 1. 验证账号和密码。
    /// 2. 创建 Session。
    /// 3. 返回原始 Token。
    pub async fn login(
        &self,
        identifier: &str,
        password: &str,
    ) -> Result<IssuedSession, AuthenticationError> {
        // 第一步：验证用户身份。
        let user = self.login_service.login(identifier, password).await?;

        // 第二步：为验证成功的用户创建 Session。
        let issued_session = self.session_service.create_session(user.id).await?;

        // 第三步：返回 Token 和 Session 信息。
        Ok(issued_session)
    }

    /// 验证客户端提交的 Token。
    pub async fn validate_token(
        &self,
        token: &str,
    ) -> Result<Option<crate::session::Session>, AuthenticationError> {
        Ok(self.session_service.validate_token(token).await?)
    }

    /// 撤销指定用户的某个 Session。
    pub async fn logout(
        &self,
        session_id: uuid::Uuid,
        user_id: uuid::Uuid,
    ) -> Result<bool, AuthenticationError> {
        Ok(self
            .session_service
            .revoke_session(session_id, user_id)
            .await?)
    }

    /// 撤销用户的所有 Session。
    pub async fn logout_all(&self, user_id: uuid::Uuid) -> Result<u64, AuthenticationError> {
        Ok(self.session_service.revoke_all_for_user(user_id).await?)
    }
}
