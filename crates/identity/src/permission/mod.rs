//! Authorization and permission management.

use thiserror::Error;

use crate::user::UserRole;

/// 当前操作需要管理员身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AuthorizationError {
    #[error("administrator role is required")]
    AdminRequired,
}

/// 检查用户是否可以执行平台管理员操作。
pub fn require_admin(role: UserRole) -> Result<(), AuthorizationError> {
    match role {
        UserRole::Admin => Ok(()),
        UserRole::Customer => Err(AuthorizationError::AdminRequired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_can_pass_admin_check() {
        assert_eq!(require_admin(UserRole::Admin), Ok(()));
    }

    #[test]
    fn customer_cannot_pass_admin_check() {
        assert_eq!(
            require_admin(UserRole::Customer),
            Err(AuthorizationError::AdminRequired)
        );
    }
}
