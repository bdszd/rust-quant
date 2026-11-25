use axum::{
    Extension, Json,
    extract::{ConnectInfo, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use std::net::SocketAddr;
use identity::{
    auth::{AuthenticationError, LoginError, RegistrationError},
    user::{NewUser, User, UserRepository, UserStatus},
};
use serde::{Deserialize, Serialize};

use crate::{
    app::state::AppState,
    http::{
        error::{ApiError, api_error},
        middleware::authentication::CurrentUser,
    },
};

/// 用户提交的登录信息。
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// 用户名或邮箱。
    pub identifier: String,
    /// 用户密码。
    pub password: String,
}

/// 用户注册信息。
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
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

/// 对外公开的用户基本信息，不包含密码凭据或 Token。
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: uuid::Uuid,
    pub username: String,
    pub email: String,
    pub status: &'static str,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        let status = match user.status {
            UserStatus::Pending => "pending",
            UserStatus::Active => "active",
            UserStatus::Disabled => "disabled",
        };
        Self {
            id: user.id,
            username: user.username,
            email: user.email,
            status,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    pub revoked: bool,
}

#[derive(Debug, Serialize)]
pub struct LogoutAllResponse {
    pub revoked_sessions: u64,
}

/// 用户登录接口。
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(client): ConnectInfo<SocketAddr>,
    Json(payload): Json<LoginRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    // 仅使用 TCP 连接的对端地址，不直接信任客户端可伪造的 X-Forwarded-For。
    // 测试会显式注入 ConnectInfo，线上由 Axum Listener 提供。
    let client_ip = client.ip();

    let retry_after = state
        .login_rate_limiter
        .check_and_record(&payload.identifier, client_ip)
        .await
        .map_err(|_| api_error(StatusCode::SERVICE_UNAVAILABLE, "登录服务暂时不可用"))?;

    if let Some(seconds) = retry_after {
        let response = (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, seconds.to_string())],
            Json(ApiError {
                error: "登录尝试过于频繁，请稍后再试".to_string(),
            }),
        )
            .into_response();
        return Ok(response);
    }

    let issued_session = state
        .authentication
        .login(&payload.identifier, &payload.password)
        .await
        .map_err(|error| match error {
            AuthenticationError::Login(
                LoginError::InvalidCredentials | LoginError::AccountNotActive,
            ) => api_error(StatusCode::UNAUTHORIZED, "用户名或密码错误"),
            _ => api_error(StatusCode::INTERNAL_SERVER_ERROR, "登录服务暂时不可用"),
        })?;

    state
        .login_rate_limiter
        .reset(&payload.identifier)
        .await
        .map_err(|_| api_error(StatusCode::SERVICE_UNAVAILABLE, "登录服务暂时不可用"))?;

    Ok(Json(LoginResponse {
        user_id: issued_session.session.user_id,
        session_id: issued_session.session.id,
        token: issued_session.token,
        expires_at: issued_session.session.expires_at.to_rfc3339(),
    })
    .into_response())
}

/// 用户注册接口。
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<UserResponse>), (StatusCode, Json<ApiError>)> {
    let new_user = NewUser {
        username: payload.username,
        email: payload.email,
    };

    let user = state
        .registration
        .register(&new_user, &payload.password)
        .await
        .map_err(|error| match error {
            RegistrationError::InvalidInput(message) => api_error(StatusCode::BAD_REQUEST, message),
            RegistrationError::UsernameAlreadyExists => {
                api_error(StatusCode::CONFLICT, "用户名已被使用")
            }
            RegistrationError::EmailAlreadyExists => {
                api_error(StatusCode::CONFLICT, "邮箱已被使用")
            }
            _ => api_error(StatusCode::INTERNAL_SERVER_ERROR, "注册服务暂时不可用"),
        })?;

    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

/// 退出当前登录会话。
pub async fn logout(
    State(state): State<AppState>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<LogoutResponse>, (StatusCode, Json<ApiError>)> {
    let revoked = state
        .authentication
        .logout(current_user.session_id, current_user.user_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "退出登录失败"))?;

    Ok(Json(LogoutResponse { revoked }))
}

/// 退出当前用户的所有登录会话。
pub async fn logout_all(
    State(state): State<AppState>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<LogoutAllResponse>, (StatusCode, Json<ApiError>)> {
    let revoked_sessions = state
        .authentication
        .logout_all(current_user.user_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "退出所有设备失败"))?;

    Ok(Json(LogoutAllResponse { revoked_sessions }))
}

/// 获取当前登录用户的信息。
pub async fn me(
    State(state): State<AppState>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<UserResponse>, (StatusCode, Json<ApiError>)> {
    let user = UserRepository::new(state.db)
        .find_by_id(current_user.user_id)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "读取用户信息失败"))?;

    user.map(|user| Json(UserResponse::from(user)))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "用户不存在"))
}
