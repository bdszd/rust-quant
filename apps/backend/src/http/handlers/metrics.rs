use axum::{Json, extract::State};

use crate::app::state::AppState;

pub async fn get_metrics(
    State(state): State<AppState>,
) -> Json<crate::observability::metrics::MetricsSnapshot> {
    let pool_size = state.db.size() as usize;
    let pool_idle = state.db.num_idle();

    let pool_stats = crate::observability::metrics::DatabasePoolStats {
        size: pool_size,
        idle: pool_idle,
        busy: pool_size.saturating_sub(pool_idle),
        max_connections: state.db.options().get_max_connections() as usize,
    };

    Json(state.metrics.snapshot(pool_stats))
}

/// GET /metrics：Prometheus text exposition format。
///
/// 保留 /api/metrics 供现有 JSON 调试使用；Prometheus 使用独立的标准抓取端点。
pub async fn prometheus_metrics(State(state): State<AppState>) -> (axum::http::HeaderMap, String) {
    use std::fmt::Write;

    let pool_size = state.db.size() as usize;
    let pool_idle = state.db.num_idle();
    let pool_stats = crate::observability::metrics::DatabasePoolStats {
        size: pool_size,
        idle: pool_idle,
        busy: pool_size.saturating_sub(pool_idle),
        max_connections: state.db.options().get_max_connections() as usize,
    };
    let snapshot = state.metrics.snapshot(pool_stats);

    let mut body = String::new();
    macro_rules! metric {
        ($name:literal, $help:literal, $kind:literal, $value:expr) => {{
            let _ = writeln!(body, "# HELP {} {}", $name, $help);
            let _ = writeln!(body, "# TYPE {} {}", $name, $kind);
            let _ = writeln!(body, "{} {}", $name, $value);
        }};
    }

    metric!(
        "rust_quant_uptime_seconds",
        "Backend process uptime in seconds.",
        "gauge",
        snapshot.uptime_seconds
    );
    metric!(
        "rust_quant_worker_count",
        "Configured number of workers.",
        "gauge",
        snapshot.worker_count
    );
    metric!(
        "rust_quant_worker_active",
        "Currently active workers.",
        "gauge",
        snapshot.worker_active
    );
    metric!(
        "rust_quant_rabbitmq_connected",
        "Whether at least one RabbitMQ connection is open.",
        "gauge",
        u8::from(snapshot.rabbitmq_connected)
    );
    metric!(
        "rust_quant_db_pool_connections",
        "Current PostgreSQL pool connections.",
        "gauge",
        snapshot.database_pool.size
    );
    metric!(
        "rust_quant_db_pool_idle",
        "Idle PostgreSQL pool connections.",
        "gauge",
        snapshot.database_pool.idle
    );
    metric!(
        "rust_quant_db_pool_busy",
        "Busy PostgreSQL pool connections.",
        "gauge",
        snapshot.database_pool.busy
    );
    metric!(
        "rust_quant_db_pool_max_connections",
        "Maximum PostgreSQL pool connections.",
        "gauge",
        snapshot.database_pool.max_connections
    );

    metric!(
        "rust_quant_tasks_accepted_total",
        "Total accepted backtest tasks since process start.",
        "counter",
        snapshot.tasks.accepted
    );
    metric!(
        "rust_quant_tasks_succeeded_total",
        "Total succeeded backtest tasks since process start.",
        "counter",
        snapshot.tasks.succeeded
    );
    metric!(
        "rust_quant_tasks_failed_total",
        "Total failed backtest tasks since process start.",
        "counter",
        snapshot.tasks.failed
    );
    metric!(
        "rust_quant_tasks_duplicate_total",
        "Total duplicate task deliveries since process start.",
        "counter",
        snapshot.tasks.duplicate
    );

    metric!(
        "rust_quant_rabbitmq_publish_total",
        "Total RabbitMQ publish attempts.",
        "counter",
        snapshot.messaging.publish_total
    );
    metric!(
        "rust_quant_rabbitmq_publish_failed_total",
        "Total RabbitMQ publish failures.",
        "counter",
        snapshot.messaging.publish_failed
    );
    metric!(
        "rust_quant_rabbitmq_publish_confirm_failed_total",
        "Total RabbitMQ publisher confirm failures.",
        "counter",
        snapshot.messaging.publish_confirm_failed
    );
    metric!(
        "rust_quant_rabbitmq_consume_total",
        "Total RabbitMQ message deliveries.",
        "counter",
        snapshot.messaging.consume_total
    );
    metric!(
        "rust_quant_rabbitmq_ack_total",
        "Total successful RabbitMQ acknowledgements.",
        "counter",
        snapshot.messaging.ack_total
    );
    metric!(
        "rust_quant_rabbitmq_ack_failed_total",
        "Total RabbitMQ acknowledgement failures.",
        "counter",
        snapshot.messaging.ack_failed
    );
    metric!(
        "rust_quant_rabbitmq_reject_total",
        "Total rejected RabbitMQ messages.",
        "counter",
        snapshot.messaging.reject_total
    );

    let _ = writeln!(
        body,
        "# HELP rust_quant_stage_duration_window_ms Rolling-window stage duration statistics in milliseconds."
    );
    let _ = writeln!(body, "# TYPE rust_quant_stage_duration_window_ms gauge");
    for (stage, stats) in snapshot.stages {
        for (stat, value) in [
            ("avg", stats.avg_ms),
            ("min", stats.min_ms),
            ("max", stats.max_ms),
            ("p50", stats.p50_ms),
            ("p95", stats.p95_ms),
            ("p99", stats.p99_ms),
        ] {
            let _ = writeln!(
                body,
                "rust_quant_stage_duration_window_ms{{stage=\"{}\",stat=\"{}\"}} {}",
                stage, stat, value
            );
        }
    }

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    (headers, body)
}
