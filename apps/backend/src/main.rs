mod api;
mod backtest;
mod dispatcher;
mod state;
mod storage;
mod tasks;
mod worker;

use std::{env, path::PathBuf, sync::Arc};

use axum::{
    Router,
    routing::{get, post},
};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tower_http::cors::CorsLayer;

use dispatcher::dispatcher_loop;
use state::AppState;
use tasks::BacktestJob;
use worker::worker_loop;

const WORKER_COUNT: usize = 2;
const QUEUE_CAPACITY: usize = 16;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化数据目录。
    let data_dir = env::var_os("RUST_QUANT_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("data")
        });

    std::fs::create_dir_all(&data_dir)?;

    // 2. 连接 PostgreSQL 并执行数据库迁移。
    let database_url = env::var("DATABASE_URL")
        .expect("必须设置 DATABASE_URL，例如 postgres://postgres:postgres@localhost:5432/rust_quant");

    let db = storage::connect(&database_url).await?;
    sqlx::migrate!("./migrations").run(&db).await?;

    // 3. 恢复上次进程遗留的 running 任务。
    let reset_count = storage::reset_running_tasks(&db).await?;
    println!("已恢复 {reset_count} 个未完成任务");

    // 4. 创建有界任务队列。
    let (task_sender, receiver) = mpsc::channel::<BacktestJob>(QUEUE_CAPACITY);
    let receiver = Arc::new(AsyncMutex::new(receiver));

    // 5. 启动固定数量的 Worker，并保留 JoinHandle。
    let mut worker_handles = Vec::with_capacity(WORKER_COUNT);

    for worker_id in 0..WORKER_COUNT {
        let receiver = Arc::clone(&receiver);
        let db = db.clone();

        let handle = tokio::spawn(async move {
            println!("Worker {worker_id} 已启动");
            worker_loop(receiver, db).await;
            println!("Worker {worker_id} 已退出");
        });

        worker_handles.push(handle);
    }

    // 6. 启动 Dispatcher。
    let (dispatcher_shutdown_tx, dispatcher_shutdown_rx) = oneshot::channel();
    let db_for_dispatch = db.clone();

    let dispatcher_handle = tokio::spawn(async move {
        dispatcher_loop(db_for_dispatch, task_sender, dispatcher_shutdown_rx).await;
    });

    // 7. 创建应用状态。
    let state = AppState { data_dir, db };

    // 8. 注册 HTTP 路由。
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/datasets", get(api::list_datasets))
        .route("/api/backtests", post(api::create_backtest))
        .route("/api/backtests/{id}", get(api::get_backtest))
        .with_state(state)
        .layer(CorsLayer::permissive());

    // 9. 启动 HTTP 服务，监听 Ctrl+C。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000").await?;
    println!("Backend listening on http://127.0.0.1:8000");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // 10. HTTP 服务退出后，等待 Worker 完成剩余任务。
    println!("HTTP 服务已关闭");
    let _ = dispatcher_shutdown_tx.send(());

    if let Err(error) = dispatcher_handle.await {
        eprintln!("Dispatcher 异常退出：{error}");
    }

    println!("等待 Worker 完成剩余任务……");
    for handle in worker_handles {
        if let Err(error) = handle.await {
            eprintln!("Worker 线程异常退出：{error}");
        }
    }

    println!("所有 Worker 已退出，程序正常结束");
    Ok(())
}

/// 等待 Ctrl+C 信号。
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("监听 Ctrl+C 信号失败：{error}");
    }

    println!("收到 Ctrl+C，开始优雅关闭 HTTP 服务");
}
