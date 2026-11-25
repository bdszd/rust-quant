use chrono::{DateTime, Utc};
use uuid::Uuid;

/// 用户认证凭据。
///
/// 当前版本只支持密码认证。
/// password_hash 保存 Argon2 编码后的哈希字符串。
#[derive(Debug, Clone)]
pub struct UserCredential {
    pub user_id: Uuid,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 创建用户密码凭据时需要的数据。
#[derive(Debug, Clone)]
pub struct NewUserCredential {
    pub user_id: Uuid,
    pub password_hash: String,
}
