use chrono::{DateTime, Utc};
use uuid::Uuid;

/// 用户账号状态。
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

/// 平台用户角色。
///
/// Customer 是所有新注册用户的默认角色；Admin 只能通过受控的管理流程授予。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRole {
    Customer,
    Admin,
}

impl UserRole {
    /// 将角色转换为数据库使用的字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Customer => "customer",
            Self::Admin => "admin",
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
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 创建用户时需要提供的信息。
///
/// 不包含 role 字段，避免注册调用方自行指定管理员角色。
#[derive(Debug, Clone)]
pub struct NewUser {
    pub username: String,
    pub email: String,
}
