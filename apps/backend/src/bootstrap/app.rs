use std::{env, sync::Arc};

use identity::auth::{AuthenticationService, RegistrationService};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    app::{router::build_router, state::AppState},
    infrastructure::{
        database::postgres_pool as storage, security::login_rate_limiter::LoginRateLimiter,
    },
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
            infrastructure::postgres_catalog::PgDatasetCatalog,
        },
    },
    observability::metrics::Metrics,
    workers::{backtest_worker::worker_loop, publisher::publisher_loop},
};

const DEFAULT_WORKER_COUNT: usize = 2;

/// 组装并启动 Backend 的 HTTP 服务、Publisher 和 Worker。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let instance_id =
        env::var("RUST_QUANT_INSTANCE_ID").unwrap_or_else(|_| Uuid::new_v4().to_string());
    if instance_id.trim().is_empty() {
        return Err("RUST_QUANT_INSTANCE_ID 不能为空".into());
    }

    let worker_count = parse_usize_env("RUST_QUANT_WORKER_COUNT", DEFAULT_WORKER_COUNT)?;
    let run_migrations = parse_bool_env("RUST_QUANT_RUN_MIGRATIONS", true)?;
    let migrate_only = parse_bool_env("RUST_QUANT_MIGRATE_ONLY", false)?;

    let database_url = env::var("DATABASE_URL").expect(
        "必须设置 DATABASE_URL，例如 postgres://postgres:postgres@localhost:5433/rust_quant",
    );
    let db = storage::connect(&database_url).await?;
    if migrate_only && !run_migrations {
        return Err(
            "RUST_QUANT_MIGRATE_ONLY=true 时不能同时设置 RUST_QUANT_RUN_MIGRATIONS=false".into(),
        );
    }

    if run_migrations {
        // 本地开发默认自动迁移。部署时可使用 RUST_QUANT_MIGRATE_ONLY=true
        // 单独执行迁移，再让所有服务实例设置 RUST_QUANT_RUN_MIGRATIONS=false。
        sqlx::migrate!("./migrations").run(&db).await?;
    } else {
        println!("实例 {instance_id} 跳过启动时数据库迁移");
    }

    if migrate_only {
        println!("数据库迁移已完成，按 RUST_QUANT_MIGRATE_ONLY 配置退出");
        return Ok(());
    }

    let amqp_addr = env::var("AMQP_ADDR")
        .expect("必须设置 AMQP_ADDR，例如 amqp://rust_quant:rust_quant_dev@localhost:5673/%2f");

    let repository = Arc::new(PgBacktestRepository::new(db.clone()));
    let publisher_repository: Arc<dyn OutboxRepository> = repository.clone();
    let worker_repository: Arc<dyn BacktestExecutionRepository> = repository.clone();
    let application_repository: Arc<dyn BacktestRepository> = repository.clone();
    let worker_execution_service = Arc::new(BacktestExecutionService::new(worker_repository));

    let market_data_repository: Arc<dyn market_data::MarketDataRepository> =
        Arc::new(market_data::PostgresRepository::new(db.clone()));
    let dataset_catalog = Arc::new(PgDatasetCatalog::new(market_data_repository.clone()));

    let backtest = Arc::new(BacktestApplicationService::new(
        application_repository,
        market_data_repository.clone(),
    ));
    let datasets = Arc::new(DatasetApplicationService::new(dataset_catalog));

    let metrics = Arc::new(Metrics::new(worker_count));
    let worker_executor: Arc<dyn BacktestExecutor> =
        Arc::new(BacktestCoreExecutor::new(market_data_repository, metrics.clone()));
    let authentication = AuthenticationService::new(db.clone());
    let registration = RegistrationService::new(db.clone());
    let login_rate_limiter = LoginRateLimiter::new(db.clone());

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let publisher_handle = {
        let shutdown = shutdown_rx.clone();
        let metrics = metrics.clone();
        let amqp_addr = amqp_addr.clone();
        tokio::spawn(async move {
            publisher_loop(publisher_repository, amqp_addr, shutdown, metrics).await;
        })
    };

    let mut worker_handles = Vec::with_capacity(worker_count);
    for worker_id in 0..worker_count {
        let execution_service = worker_execution_service.clone();
        let executor = worker_executor.clone();
        let amqp_addr = amqp_addr.clone();
        let shutdown = shutdown_rx.clone();
        let metrics = metrics.clone();
        let instance_id = instance_id.clone();

        worker_handles.push(tokio::spawn(async move {
            worker_loop(
                worker_id,
                instance_id,
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
        registration,
        login_rate_limiter,
    };
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    println!(
        "Backend 实例 {instance_id} listening on http://0.0.0.0:8000，Worker 数量：{worker_count}"
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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

fn parse_usize_env(name: &str, default: usize) -> Result<usize, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn parse_bool_env(name: &str, default: bool) -> Result<bool, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => Err(format!("{name} 必须是 true/false、1/0 或 yes/no").into()),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("监听 Ctrl+C 信号失败：{error}");
    }
    println!("收到 Ctrl+C，开始优雅关闭 HTTP 服务");
}
