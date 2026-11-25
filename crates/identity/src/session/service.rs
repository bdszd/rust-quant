use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use super::{
    model::{NewSession, Session},
    repository::{SessionRepository, SessionRepositoryError},
};

/// Session 默认有效期：30 天。
const SESSION_TTL_DAYS: i64 = 30;

/// 创建 Session 后返回的数据。
///
/// token 是原始 Token，只在创建时返回一次。
/// session 是数据库中的 Session 信息。
#[derive(Debug, Clone)]
pub struct IssuedSession {
    pub token: String,
    pub session: Session,
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session repository error: {0}")]
    Repository(#[from] SessionRepositoryError),
}

/// Session 管理服务。
#[derive(Debug, Clone)]
pub struct SessionService {
    repository: SessionRepository,
}

impl SessionService {
    pub fn new(repository: SessionRepository) -> Self {
        Self { repository }
    }

    /// 为用户创建一个新的 Session。
    pub async fn create_session(&self, user_id: Uuid) -> Result<IssuedSession, SessionError> {
        // 1. 生成 32 字节的安全随机数。
        let mut random_bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut random_bytes);

        // 2. 将随机字节编码为 URL 安全的 Base64 字符串。
        let token = URL_SAFE_NO_PAD.encode(random_bytes);

        // 3. 计算 Token 的 SHA-256 哈希。
        let token_hash = Self::hash_token(&token);

        // 4. 设置过期时间。
        let expires_at = Utc::now() + Duration::days(SESSION_TTL_DAYS);

        // 5. 将 Token 哈希写入数据库。
        let new_session = NewSession {
            user_id,
            token_hash,
            expires_at,
        };

        let session = self.repository.create(&new_session).await?;

        // 6. 只向调用方返回原始 Token 一次。
        Ok(IssuedSession { token, session })
    }

    /// 根据原始 Token 验证 Session。
    pub async fn validate_token(&self, token: &str) -> Result<Option<Session>, SessionError> {
        if token.is_empty() {
            return Ok(None);
        }

        let token_hash = Self::hash_token(token);

        let session = self
            .repository
            .find_active_by_token_hash(&token_hash)
            .await?;

        Ok(session)
    }

    /// 撤销指定 Session。
    pub async fn revoke_session(
        &self,
        session_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, SessionError> {
        Ok(self.repository.revoke(session_id, user_id).await?)
    }

    /// 撤销用户的所有 Session。
    pub async fn revoke_all_for_user(&self, user_id: Uuid) -> Result<u64, SessionError> {
        Ok(self.repository.revoke_all_for_user(user_id).await?)
    }

    /// 计算 Token 的 SHA-256 哈希，并转换为十六进制字符串。
    fn hash_token(token: &str) -> String {
        let digest = Sha256::digest(token.as_bytes());

        let mut result = String::with_capacity(64);

        for byte in digest {
            use std::fmt::Write;

            write!(&mut result, "{byte:02x}").expect("writing to String cannot fail");
        }

        result
    }
}
