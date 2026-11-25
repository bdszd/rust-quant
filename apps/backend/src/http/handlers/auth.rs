use axum::{
    Json,
    extract::State,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use identity::auth::{AuthenticationError, LoginError};

use crate::{
    app::state::AppState,
    http::error::{ApiError, api_error},
};

/// 用户提交的登录信息。
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// 用户名或邮箱。
    pub identifier: String,

    /// 用户密码。
    pub password: String,
}

/// 登录成功后返回给客户端的信息。
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user_id: uuid::Uuid,
    pub session_id: uuid::Uuid,
    pub token: String,
    pub expires_at: String,
}

/// 用户登录接口。
pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<ApiError>)> {
    // 1. 调用 Identity 模块验证账号密码并创建 Session。
    let issued_session = state
        .authentication
        .login(&payload.identifier, &payload.password)
        .await
        .map_err(|error| match error {
            // 账号不存在或密码错误。
            AuthenticationError::Login(LoginError::InvalidCredentials) => {
                api_error(StatusCode::UNAUTHORIZED, "用户名或密码错误")
            }

            // 账号还没有激活，或者当前不可用。
            AuthenticationError::Login(LoginError::AccountNotActive) => {
                api_error(StatusCode::FORBIDDEN, "账号尚未激活或当前不可用")
            }

            // 其他错误属于服务器内部问题。
            _ => api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "登录服务暂时不可用",
            ),
        })?;

    // 2. 整理登录成功后的返回数据。
    let response = LoginResponse {
        user_id: issued_session.session.user_id,
        session_id: issued_session.session.id,
        token: issued_session.token,
        expires_at: issued_session.session.expires_at.to_rfc3339(),
    };

    // 3. 返回 JSON。
    Ok(Json(response))
}