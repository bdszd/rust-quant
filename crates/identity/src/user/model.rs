use chrono::{DateTime, Utc};
use uuid::Uuid;

/// 用户状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Pending,
    Active,
    Disabled,
}

impl UserStatus {
    /// 将用户状态转换为数据库使用的字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }
}

/// 用户基本信息。
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub status: UserStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 创建用户时需要提供的信息。
#[derive(Debug, Clone)]
pub struct NewUser {
    pub username: String,
    pub email: String,
}
