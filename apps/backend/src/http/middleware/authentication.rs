use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};
use identity::{
    user::{UserRepository, UserStatus},
};
use uuid::Uuid;

use crate::{
    app::state::AppState,
    http::error::{ApiError, api_error},
};

/// 当前请求对应的登录用户。
///
/// 角色从数据库实时读取，不从客户端请求参数或 Token 内容中信任角色，
/// 因此管理员角色被撤销后，下一次请求就会按新角色进行判断。
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub role: identity::user::UserRole,
}

/// 检查请求是否携带有效的登录 Token，并加载当前用户信息。
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let Some(token) = token else {
        return Err(api_error(StatusCode::UNAUTHORIZED, "请先登录"));
    };

    let session = state
        .authentication
        .validate_token(token)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "认证服务暂时不可用"))?;

    let Some(session) = session else {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "登录状态已失效，请重新登录",
        ));
    };

    // 每次受保护请求都从数据库读取角色，避免角色变更后旧 Token 仍保留管理员权限。
    let user = UserRepository::new(state.db.clone())
        .find_by_id(session.user_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "读取用户信息失败"))?;

    let Some(user) = user else {
        return Err(api_error(StatusCode::UNAUTHORIZED, "用户不存在或登录状态已失效"));
    };

    if user.status != UserStatus::Active {
        return Err(api_error(StatusCode::UNAUTHORIZED, "账号当前不可用"));
    }

    let current_user = CurrentUser {
        user_id: user.id,
        session_id: session.id,
        role: user.role,
    };

    request.extensions_mut().insert(current_user);

    Ok(next.run(request).await)
}
