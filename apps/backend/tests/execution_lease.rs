use quant_backend::modules::backtest::infrastructure::postgres_repository::PgBacktestRepository;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn create_running_attempt(pool: &PgPool, expired: bool) -> (Uuid, Uuid, Uuid) {
    let task_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let attempt_id = Uuid::new_v4();
    let worker_id = "lease-test-worker";

    sqlx::query("INSERT INTO backtest_tasks (id, status) VALUES ($1, 'running')")
        .bind(task_id)
        .execute(pool)
        .await
        .unwrap();

        let config = json!({
        "dataset": "00000000-0000-0000-0000-000000000002",
        "symbols": ["000001.SZ"],
        "start_date": "2024-01-01",
        "end_date": "2024-12-31",
        "initial_cash": 100000.0,
        "commission_rate": 0.0003,
        "slippage_rate": 0.001,
        "lot_size": 100,
        "stamp_duty_rate": 0.001,
        "min_commission": 5.0,
        "execution_mode": "next_open",
        "strategy": { "type": "buy_and_hold" }
    });

    sqlx::query("INSERT INTO backtest_jobs (id, task_id, config) VALUES ($1, $2, $3)")
        .bind(job_id)
        .bind(task_id)
        .bind(config)
        .execute(pool)
        .await
        .unwrap();

    let expiry = if expired {
        "NOW() - INTERVAL '1 second'"
    } else {
        "NOW() + INTERVAL '60 seconds'"
    };
    
    let query = format!(
        "INSERT INTO backtest_task_attempts \
         (id, task_id, attempt_number, status, worker_id, lease_expires_at) \
         VALUES ($1, $2, 1, 'running', $3, {expiry})"
    );
    sqlx::query(&query)
        .bind(attempt_id)
        .bind(task_id)
        .bind(worker_id)
        .execute(pool)
        .await
        .unwrap();

    (task_id, job_id, attempt_id)
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_attempt_cannot_heartbeat(pool: PgPool) {
    let (_, _, attempt_id) = create_running_attempt(&pool, true).await;
    let repository = PgBacktestRepository::new(pool);

    let renewed = repository
        .heartbeat_attempt(attempt_id, "lease-test-worker", 60)
        .await
        .unwrap();

    assert!(!renewed, "an expired lease must not be renewable");
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_attempt_cannot_complete_success_or_write_result(pool: PgPool) {
    let (task_id, _, attempt_id) = create_running_attempt(&pool, true).await;
    let repository = PgBacktestRepository::new(pool.clone());

    let result = repository
        .complete_success(
            attempt_id,
            task_id,
            "lease-test-worker",
            &json!({ "total_return": 0.1 }),
            100,
        )
        .await;

    assert!(result.unwrap_err().contains("不再拥有该执行 Attempt"));
    let result_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM backtest_results WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(result_count, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_attempt_cannot_complete_failure(pool: PgPool) {
    let (task_id, _, attempt_id) = create_running_attempt(&pool, true).await;
    let repository = PgBacktestRepository::new(pool.clone());

    let result = repository
        .complete_failure(
            attempt_id,
            task_id,
            "lease-test-worker",
            "late failure",
            Some(100),
        )
        .await;

    assert!(result.unwrap_err().contains("不再拥有该执行 Attempt"));
    let status: String = sqlx::query_scalar("SELECT status FROM backtest_tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "running");
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_attempt_can_be_taken_over_and_old_worker_is_rejected(pool: PgPool) {
    let (task_id, job_id, old_attempt_id) = create_running_attempt(&pool, true).await;
    let repository = PgBacktestRepository::new(pool.clone());

    let outcome = repository
        .claim_execution(task_id, job_id, "replacement-worker", 60)
        .await
        .unwrap();

    let new_attempt_id = match outcome {
        quant_backend::modules::backtest::application::ports::ClaimExecutionOutcome::Claimed(
            job,
        ) => job.attempt.id,
        other => panic!("expected a new attempt, got {other:?}"),
    };

    let old_result = repository
        .complete_success(
            old_attempt_id,
            task_id,
            "lease-test-worker",
            &json!({ "total_return": 0.1 }),
            100,
        )
        .await;
    assert!(old_result.unwrap_err().contains("不再拥有该执行 Attempt"));

    let old_status: String =
        sqlx::query_scalar("SELECT status FROM backtest_task_attempts WHERE id = $1")
            .bind(old_attempt_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(old_status, "interrupted");

    let new_status: String =
        sqlx::query_scalar("SELECT status FROM backtest_task_attempts WHERE id = $1")
            .bind(new_attempt_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(new_status, "running");
}

#[sqlx::test(migrations = "./migrations")]
async fn active_attempt_can_complete_success(pool: PgPool) {
    let (task_id, _, attempt_id) = create_running_attempt(&pool, false).await;
    let repository = PgBacktestRepository::new(pool.clone());

    repository
        .complete_success(
            attempt_id,
            task_id,
            "lease-test-worker",
            &json!({ "total_return": 0.1 }),
            100,
        )
        .await
        .unwrap();

    let task_status: String = sqlx::query_scalar("SELECT status FROM backtest_tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(task_status, "succeeded");
}
