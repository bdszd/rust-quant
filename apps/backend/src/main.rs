mod app;
mod http;
mod infrastructure;
mod modules;

use infrastructure::database::backtest_repository as storage;

mod observability;
mod tasks;

use std::{env, path::PathBuf, sync::Arc};

use app::router::build_router;
use identity::auth::AuthenticationService;
use tokio::sync::watch;

use app::state::AppState;
use observability::metrics::Metrics;
use tasks::publisher::publisher_loop;
use tasks::worker::worker_loop;

const WORKER_COUNT: usize = 2;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化清洗后行情数据目录。
    let data_dir = env::var_os("RUST_QUANT_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("data")
                .join("cleaned")
                .join("tushare")
        });

    std::fs::create_dir_all(&data_dir)?;

    // 2. 连接 PostgreSQL 并执行数据库迁移。
    let database_url = env::var("DATABASE_URL").expect(
        "必须设置 DATABASE_URL，例如 postgres://postgres:postgres@localhost:5433/rust_quant",
    );

    let amqp_addr = env::var("AMQP_ADDR")
        .expect("必须设置 AMQP_ADDR，例如 amqp://rust_quant:rust_quant_dev@localhost:5673/%2f");

    let db = storage::connect(&database_url).await?;
    sqlx::migrate!("./migrations").run(&db).await?;

    // 3. 恢复上次进程遗留的 running 任务，并重新放回 Outbox。
    let reset_count = storage::reset_running_tasks(&db).await?;
    println!("已恢复 {reset_count} 个未完成任务");

    // 4. 初始化进程级性能统计。
    let metrics = Arc::new(Metrics::new(WORKER_COUNT));

    // 5. 创建认证服务。
    // 认证服务和 Backend 使用同一个 PostgreSQL 连接池。
    let authentication = AuthenticationService::new(db.clone());

    // 6. 创建统一关闭信号。
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // 7. 启动 Outbox Publisher。
    let publisher_db = db.clone();
    let publisher_amqp_addr = amqp_addr.clone();
    let publisher_shutdown = shutdown_rx.clone();
    let publisher_metrics = metrics.clone();

    let publisher_handle = tokio::spawn(async move {
        publisher_loop(
            publisher_db,
            publisher_amqp_addr,
            publisher_shutdown,
            publisher_metrics,
        )
        .await;
    });

    // 8. 启动固定数量的 RabbitMQ Consumer。
    let mut worker_handles = Vec::with_capacity(WORKER_COUNT);

    for worker_id in 0..WORKER_COUNT {
        let worker_db = db.clone();
        let worker_amqp_addr = amqp_addr.clone();
        let worker_shutdown = shutdown_rx.clone();
        let worker_metrics = metrics.clone();

        let handle = tokio::spawn(async move {
            worker_loop(
                worker_id,
                worker_db,
                worker_amqp_addr,
                worker_shutdown,
                worker_metrics,
            )
            .await;
        });

        worker_handles.push(handle);
    }

    // 9. 创建应用状态。
    let state = AppState {
        data_dir,
        db,
        metrics,
        authentication,
    };

    // 10. 创建应用 Router。
    let app = build_router(state);

    // 11. 启动 HTTP 服务，监听 Ctrl+C。
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    println!("Backend listening on http://0.0.0.0:8000");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // 12. 停止 Publisher 和 Consumer。
    println!("HTTP 服务已关闭，通知后台任务退出");
    let _ = shutdown_tx.send(true);

    if let Err(error) = publisher_handle.await {
        eprintln!("Outbox Publisher 异常退出：{error}");
    }

    println!("等待 Worker 完成当前任务……");
    for handle in worker_handles {
        if let Err(error) = handle.await {
            eprintln!("Worker 异常退出：{error}");
        }
    }

    println!("所有后台任务已退出，程序正常结束");
    Ok(())
}

/// 等待 Ctrl+C 信号。
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("监听 Ctrl+C 信号失败：{error}");
    }

    println!("收到 Ctrl+C，开始优雅关闭 HTTP 服务");
}