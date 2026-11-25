use chrono::{DateTime, Utc};
use uuid::Uuid;

/// 用户会话。
///
/// 注意：这里不保存原始 Session Token，
/// 只保存 Token 的哈希值。
#[derive(Debug, Clone)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// 创建 Session 时需要的数据。
#[derive(Debug, Clone)]
pub struct NewSession {
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}
