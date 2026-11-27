use axum::{Extension, extract::Request, http::StatusCode, middleware::Next, response::Response};
use identity::permission::{AuthorizationError, require_admin};

use crate::http::{
    error::{ApiError, api_error},
    middleware::authentication::CurrentUser,
};

/// 只允许平台管理员访问后续路由。
///
/// 应当与 require_auth 一起使用，并确保认证中间件先执行。
pub async fn require_admin_role(
    Extension(current_user): Extension<CurrentUser>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, axum::Json<ApiError>)> {
    require_admin(current_user.role).map_err(|error| match error {
        AuthorizationError::AdminRequired => {
            api_error(StatusCode::FORBIDDEN, "没有执行此操作的权限")
        }
    })?;

    Ok(next.run(request).await)
}
