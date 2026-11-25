use quant_backend::modules::backtest::infrastructure::postgres_repository::PgBacktestRepository;
use sqlx::PgPool;
use uuid::Uuid;

async fn create_user(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, username, email, status) VALUES ($1, $2, $3, 'active')")
        .bind(id)
        .bind(username)
        .bind(format!("{username}@example.test"))
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn create_task(pool: &PgPool, user_id: Uuid) -> Uuid {
    let task_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();

    sqlx::query("INSERT INTO backtest_tasks (id, user_id, status) VALUES ($1, $2, 'queued')")
        .bind(task_id)
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();

    let config = serde_json::json!({
        "dataset": "000001.SZ/20240101_20241231",
        "initial_cash": 100000.0,
        "commission_rate": 0.0003,
        "slippage_rate": 0.001,
        "lot_size": 100,
        "stamp_duty_rate": 0.0005,
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

    task_id
}

#[sqlx::test(migrations = "./migrations")]
async fn users_can_only_list_and_read_their_own_backtests(pool: PgPool) {
    let user_a = create_user(&pool, "owner_a").await;
    let user_b = create_user(&pool, "owner_b").await;
    let task_a = create_task(&pool, user_a).await;
    let task_b = create_task(&pool, user_b).await;

    let repository = PgBacktestRepository::new(pool);

    let tasks_a = repository.list_tasks(user_a, 50).await.unwrap();
    assert_eq!(tasks_a.len(), 1);
    assert_eq!(tasks_a[0].id, task_a);

    let own_task = repository.find_task(task_a, user_a).await.unwrap();
    assert!(own_task.is_some());

    let other_users_task = repository.find_task(task_b, user_a).await.unwrap();
    assert!(other_users_task.is_none());

    let tasks_b = repository.list_tasks(user_b, 50).await.unwrap();
    assert_eq!(tasks_b.len(), 1);
    assert_eq!(tasks_b[0].id, task_b);
}
