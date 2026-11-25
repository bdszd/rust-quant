use axum::{Router, middleware, routing::get};
use tower_http::cors::CorsLayer;

use crate::{
    app::state::AppState,
    http::{
        handlers,
        middleware::authentication::require_auth,
    },
};

pub fn build_router(state: AppState) -> Router {
    // 需要登录才能访问的接口。
    let protected_routes = Router::new()
        .route(
            "/api/backtests",
            get(handlers::backtests::list_backtests)
                .post(handlers::backtests::create_backtest),
        )
        .route(
            "/api/backtests/{id}",
            get(handlers::backtests::get_backtest),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    // 不需要登录的接口。
    Router::new()
        .route("/api/health", get(handlers::health::health))
        .route("/api/metrics", get(handlers::metrics::get_metrics))
        .route("/metrics", get(handlers::metrics::prometheus_metrics))
        .route("/api/datasets", get(handlers::datasets::list_datasets))
        // 登录接口不能放进 protected_routes，否则用户还没登录就无法登录。
        .route("/api/auth/login", axum::routing::post(handlers::auth::login))
        .merge(protected_routes)
        .with_state(state)
        .layer(CorsLayer::permissive())
}