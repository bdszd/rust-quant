use std::{env, time::Duration};

use axum::{
    Router,
    http::{HeaderValue, Method, header},
    middleware,
    routing::get,
};
use tower_http::cors::CorsLayer;

use crate::{
    app::state::AppState,
    http::{handlers, middleware::authentication::require_auth},
};

pub fn build_router(state: AppState) -> Router {
    // 需要登录才能访问的接口。
    let protected_routes = Router::new()
        .route(
            "/api/backtests",
            get(handlers::backtests::list_backtests).post(handlers::backtests::create_backtest),
        )
        .route(
            "/api/backtests/{id}",
            get(handlers::backtests::get_backtest),
        )
        .route(
            "/api/auth/logout",
            axum::routing::post(handlers::auth::logout),
        )
        .route(
            "/api/auth/logout-all",
            axum::routing::post(handlers::auth::logout_all),
        )
        .route("/api/auth/me", get(handlers::auth::me))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    // 不需要登录的接口。
    Router::new()
        .route("/api/health", get(handlers::health::health))
        .route("/api/metrics", get(handlers::metrics::get_metrics))
        .route("/metrics", get(handlers::metrics::prometheus_metrics))
        .route("/api/datasets", get(handlers::datasets::list_datasets))
        // 登录接口不能放进 protected_routes，否则用户还没登录就无法登录。
        .route(
            "/api/auth/login",
            axum::routing::post(handlers::auth::login),
        )
        .route(
            "/api/auth/register",
            axum::routing::post(handlers::auth::register),
        )
        .merge(protected_routes)
        .with_state(state)
        .layer(cors_layer())
}

/// 只允许配置列表中的网页来源跨域访问。
///
/// 未配置时仅允许本地 Vite 开发地址；空配置或非法 Origin 会在启动时失败，
/// 不会退回到允许所有来源。
fn cors_layer() -> CorsLayer {
    let configured = env::var("RUST_QUANT_CORS_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:5173".to_string());

    let origins: Vec<HeaderValue> = configured
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            origin.parse::<HeaderValue>().unwrap_or_else(|_| {
                panic!("RUST_QUANT_CORS_ALLOWED_ORIGINS 包含非法 Origin: {origin}")
            })
        })
        .collect();

    assert!(
        !origins.is_empty(),
        "RUST_QUANT_CORS_ALLOWED_ORIGINS 不能为空；请配置明确的前端 Origin"
    );

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .expose_headers([header::RETRY_AFTER])
        .max_age(Duration::from_secs(3600))
}
