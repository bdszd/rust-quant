use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};

use crate::{
    app::state::AppState,
    http::error::{ApiError, api_error},
};

use uuid::Uuid;

/// 当前请求对应的登录用户。
///
/// 认证成功后，中间件会把这个信息放进请求中，
/// 后面的 Handler 就可以直接读取。
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user_id: Uuid,
}

/// 检查请求是否携带有效的登录 Token。
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    // 1. 从 HTTP 请求头中读取 Authorization。
    let token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    // 2. 没有携带 Token，直接拒绝请求。
    let Some(token) = token else {
        return Err(api_error(StatusCode::UNAUTHORIZED, "请先登录"));
    };

    // 3. 调用 Identity 模块，检查 Token 是否有效。
    let session = state
        .authentication
        .validate_token(token)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "认证服务暂时不可用"))?;

    // 4. Token 无效、已过期或者已经退出登录。
    let Some(session) = session else {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "登录状态已失效，请重新登录",
        ));
    };

    // 5. 把当前用户的身份信息放进请求中。
    let current_user = CurrentUser {
        user_id: session.user_id,
    };

    request.extensions_mut().insert(current_user);

    // 6. 身份验证成功，继续执行后面的接口。
    Ok(next.run(request).await)
}
