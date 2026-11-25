use std::{env, path::PathBuf, sync::Arc};

use identity::auth::AuthenticationService;
use tokio::sync::watch;

use crate::{
    app::{router::build_router, state::AppState},
    infrastructure::database::backtest_repository as storage,
    modules::{
        backtest::{
            application::{
                BacktestApplicationService,
                execution::BacktestExecutionService,
                ports::{
                    BacktestExecutionRepository, BacktestExecutor, BacktestRepository,
                    OutboxRepository,
                },
            },
            infrastructure::{
                backtest_executor::BacktestCoreExecutor, postgres_repository::PgBacktestRepository,
            },
        },
        dataset::{
            application::DatasetApplicationService,
            infrastructure::filesystem_catalog::FilesystemDatasetCatalog,
        },
    },
    observability::metrics::Metrics,
    workers::{backtest_worker::worker_loop, publisher::publisher_loop},
};

const WORKER_COUNT: usize = 2;

/// 组装并启动 Backend 的 HTTP 服务、Publisher 和 Worker。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
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

    let database_url = env::var("DATABASE_URL").expect(
        "必须设置 DATABASE_URL，例如 postgres://postgres:postgres@localhost:5433/rust_quant",
    );
    let amqp_addr = env::var("AMQP_ADDR")
        .expect("必须设置 AMQP_ADDR，例如 amqp://rust_quant:rust_quant_dev@localhost:5673/%2f");

    let db = storage::connect(&database_url).await?;
    sqlx::migrate!("./migrations").run(&db).await?;

    let repository = Arc::new(PgBacktestRepository::new(db.clone()));
    let publisher_repository: Arc<dyn OutboxRepository> = repository.clone();
    let worker_repository: Arc<dyn BacktestExecutionRepository> = repository.clone();
    let application_repository: Arc<dyn BacktestRepository> = repository.clone();
    let worker_execution_service = Arc::new(BacktestExecutionService::new(worker_repository));

    let dataset_catalog = Arc::new(FilesystemDatasetCatalog::new(data_dir.clone()));
    let backtest = Arc::new(BacktestApplicationService::new(
        application_repository,
        dataset_catalog.clone(),
    ));
    let datasets = Arc::new(DatasetApplicationService::new(dataset_catalog));

    let metrics = Arc::new(Metrics::new(WORKER_COUNT));
    let worker_executor: Arc<dyn BacktestExecutor> =
        Arc::new(BacktestCoreExecutor::new(data_dir, metrics.clone()));
    let authentication = AuthenticationService::new(db.clone());

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let publisher_handle = {
        let shutdown = shutdown_rx.clone();
        let metrics = metrics.clone();
        let amqp_addr = amqp_addr.clone();
        tokio::spawn(async move {
            publisher_loop(publisher_repository, amqp_addr, shutdown, metrics).await;
        })
    };

    let mut worker_handles = Vec::with_capacity(WORKER_COUNT);
    for worker_id in 0..WORKER_COUNT {
        let execution_service = worker_execution_service.clone();
        let executor = worker_executor.clone();
        let amqp_addr = amqp_addr.clone();
        let shutdown = shutdown_rx.clone();
        let metrics = metrics.clone();

        worker_handles.push(tokio::spawn(async move {
            worker_loop(
                worker_id,
                execution_service,
                executor,
                amqp_addr,
                shutdown,
                metrics,
            )
            .await;
        }));
    }

    let state = AppState {
        db,
        metrics,
        backtest,
        datasets,
        authentication,
    };
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    println!("Backend listening on http://0.0.0.0:8000");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

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

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("监听 Ctrl+C 信号失败：{error}");
    }
    println!("收到 Ctrl+C，开始优雅关闭 HTTP 服务");
}
