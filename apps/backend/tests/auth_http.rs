use std::{fs, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use identity::{
    auth::{AuthenticationService, RegistrationService},
    user::{UserRepository, UserStatus},
};
use quant_backend::{
    app::{router::build_router, state::AppState},
    modules::{
        backtest::{
            application::BacktestApplicationService,
            infrastructure::postgres_repository::PgBacktestRepository,
        },
        dataset::{
            application::DatasetApplicationService,
            infrastructure::filesystem_catalog::FilesystemDatasetCatalog,
        },
    },
    observability::metrics::Metrics,
    infrastructure::security::login_rate_limiter::LoginRateLimiter,
};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

fn test_app(pool: PgPool) -> (axum::Router, std::path::PathBuf) {
    let data_dir = std::env::temp_dir().join(format!("rust-quant-auth-http-{}", Uuid::new_v4()));
    fs::create_dir_all(&data_dir).unwrap();

    let repository = Arc::new(PgBacktestRepository::new(pool.clone()));
    let backtest = Arc::new(BacktestApplicationService::new(
        repository,
        Arc::new(FilesystemDatasetCatalog::new(data_dir.clone())),
    ));
    let datasets = Arc::new(DatasetApplicationService::new(Arc::new(
        FilesystemDatasetCatalog::new(data_dir.clone()),
    )));

    let state = AppState {
        db: pool.clone(),
        metrics: Arc::new(Metrics::new(0)),
        backtest,
        datasets,
        authentication: AuthenticationService::new(pool.clone()),
        registration: RegistrationService::new(pool.clone()),
        login_rate_limiter: LoginRateLimiter::new(pool),
    };

    (build_router(state), data_dir)
}

async fn send(app: &axum::Router, mut request: Request<Body>) -> axum::response::Response {
    request.extensions_mut().insert(axum::extract::ConnectInfo(
        "127.0.0.1:40000".parse().unwrap(),
    ));
    app.clone().oneshot(request).await.unwrap()
}

fn json_request(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn auth_http_endpoints_complete_the_login_lifecycle(pool: PgPool) {
    let (app, data_dir) = test_app(pool);

    // 未登录访问受保护接口，必须在进入 Handler 前被拒绝。
    let response = send(
        &app,
        Request::builder()
            .uri("/api/auth/me")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = send(
        &app,
        Request::builder()
            .uri("/api/backtests")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 注册。
    let response = send(
        &app,
        json_request(
            "POST",
            "/api/auth/register",
            serde_json::json!({
                "username": "http_auth_user",
                "email": "http-auth@example.test",
                "password": "correct-horse-battery"
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let registered: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(registered["username"], "http_auth_user");
    assert_eq!(registered["email"], "http-auth@example.test");
    assert!(registered.get("password").is_none());

    // 登录并取得 Bearer Token。
    let response = send(
        &app,
        json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({
                "identifier": "http_auth_user",
                "password": "correct-horse-battery"
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let login: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let token = login["token"].as_str().unwrap().to_string();
    assert!(!token.is_empty());

    // /me 只能通过有效 Token 访问，并且不会返回敏感凭据。
    let response = send(
        &app,
        Request::builder()
            .uri("/api/auth/me")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let me: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(me["id"], registered["id"]);
    assert_eq!(me["username"], "http_auth_user");
    assert!(me.get("password_hash").is_none());

    // 退出当前会话后，原 Token 立即失效。
    let response = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/logout")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &app,
        Request::builder()
            .uri("/api/auth/me")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 重新登录两次，验证 HTTP 的 logout-all 会撤销该用户所有设备的会话。
    let login_payload = serde_json::json!({
        "identifier": "http_auth_user",
        "password": "correct-horse-battery"
    });
    let response = send(
        &app,
        json_request("POST", "/api/auth/login", login_payload.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let second_token: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();

    let response = send(&app, json_request("POST", "/api/auth/login", login_payload)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let third_token: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();

    let response = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/logout-all")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", second_token["token"].as_str().unwrap()),
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &app,
        Request::builder()
            .uri("/api/auth/me")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", third_token["token"].as_str().unwrap()),
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    fs::remove_dir_all(data_dir).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn duplicate_registration_returns_conflict(pool: PgPool) {
    let (app, data_dir) = test_app(pool);

    let payload = serde_json::json!({
        "username": "duplicate_http_user",
        "email": "duplicate-http@example.test",
        "password": "correct-horse-battery"
    });

    let first = send(
        &app,
        json_request("POST", "/api/auth/register", payload.clone()),
    )
    .await;
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = send(&app, json_request("POST", "/api/auth/register", payload)).await;
    assert_eq!(second.status(), StatusCode::CONFLICT);

    fs::remove_dir_all(data_dir).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn login_does_not_reveal_disabled_account_status(pool: PgPool) {
    let (app, data_dir) = test_app(pool.clone());

    let response = send(
        &app,
        json_request(
            "POST",
            "/api/auth/register",
            serde_json::json!({
                "username": "disabled_http_user",
                "email": "disabled-http@example.test",
                "password": "correct-horse-battery"
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let user = UserRepository::new(pool.clone())
        .find_by_username("disabled_http_user")
        .await
        .unwrap()
        .expect("registered user should exist");

    UserRepository::new(pool)
        .update_status(user.id, UserStatus::Disabled)
        .await
        .unwrap()
        .expect("user should exist");

    let disabled_response = send(
        &app,
        json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({
                "identifier": "disabled_http_user",
                "password": "correct-horse-battery"
            }),
        ),
    )
    .await;

    let invalid_password_response = send(
        &app,
        json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({
                "identifier": "disabled_http_user",
                "password": "wrong-password"
            }),
        ),
    )
    .await;

    assert_eq!(disabled_response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(invalid_password_response.status(), StatusCode::UNAUTHORIZED);

    let disabled_body = to_bytes(disabled_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let invalid_password_body = to_bytes(invalid_password_response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(disabled_body, invalid_password_body);

    fs::remove_dir_all(data_dir).unwrap();
}


#[sqlx::test(migrations = "./migrations")]
async fn login_is_rate_limited_after_ten_attempts(pool: PgPool) {
    let (app, data_dir) = test_app(pool);

    let registration = send(
        &app,
        json_request(
            "POST",
            "/api/auth/register",
            serde_json::json!({
                "username": "rate_limited_user",
                "email": "rate-limited@example.test",
                "password": "correct-horse-battery"
            }),
        ),
    )
    .await;
    assert_eq!(registration.status(), StatusCode::CREATED);

    for attempt in 1..=10 {
        let response = send(
            &app,
            json_request(
                "POST",
                "/api/auth/login",
                serde_json::json!({
                    "identifier": "rate_limited_user",
                    "password": "wrong-password"
                }),
            ),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "attempt {attempt} should still be allowed"
        );
    }

    let response = send(
        &app,
        json_request(
            "POST",
            "/api/auth/login",
            serde_json::json!({
                "identifier": "rate_limited_user",
                "password": "wrong-password"
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().get(header::RETRY_AFTER).is_some());

    fs::remove_dir_all(data_dir).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn cors_only_allows_configured_origins(pool: PgPool) {
    let (app, data_dir) = test_app(pool);

    let allowed = send(
        &app,
        Request::builder()
            .method("OPTIONS")
            .uri("/api/auth/login")
            .header(header::ORIGIN, "http://localhost:5173")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization,content-type")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        allowed.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "http://localhost:5173"
    );

    let denied = send(
        &app,
        Request::builder()
            .method("OPTIONS")
            .uri("/api/auth/login")
            .header(header::ORIGIN, "https://attacker.example")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(denied.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());

    fs::remove_dir_all(data_dir).unwrap();
}
