use crate::{
    backtest::execute_job,
    storage,
    tasks::{ApiBacktestResult, BacktestJob},
};
use sqlx::PgPool;
use std::{
    cmp,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(100);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);

/// 数据库操作失败后的退避等待时间。
struct RetryDelay {
    current: Duration,
}

impl RetryDelay {
    fn new() -> Self {
        Self {
            current: INITIAL_RETRY_DELAY,
        }
    }

    async fn wait(&mut self) {
        tokio::time::sleep(self.current).await;
        self.current = cmp::min(self.current.saturating_mul(2), MAX_RETRY_DELAY);
    }
}

fn duration_to_ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

async fn persist_succeeded(
    db: &PgPool,
    id: &str,
    result: &ApiBacktestResult,
    execution_duration_ms: i64,
) {
    let mut retry = RetryDelay::new();

    loop {
        match storage::mark_succeeded(db, id, result, execution_duration_ms).await {
            Ok(()) => {
                println!("任务 {id} 执行成功，结果已保存，执行耗时：{execution_duration_ms} ms");
                return;
            }
            Err(error) => {
                eprintln!("保存任务 {id} 的执行结果失败：{error}，稍后重试");
                retry.wait().await;
            }
        }
    }
}

async fn persist_failed(db: &PgPool, id: &str, message: &str, execution_duration_ms: Option<i64>) {
    let mut retry = RetryDelay::new();

    loop {
        match storage::mark_failed(db, id, message, execution_duration_ms).await {
            Ok(()) => {
                println!("任务 {id} 已记录失败状态");
                return;
            }
            Err(error) => {
                eprintln!("保存任务 {id} 的失败状态失败：{error}，稍后重试");
                retry.wait().await;
            }
        }
    }
}

pub async fn worker_loop(receiver: Arc<AsyncMutex<mpsc::Receiver<BacktestJob>>>, db: PgPool) {
    loop {
        let job = {
            let mut receiver = receiver.lock().await;
            receiver.recv().await
        };

        let Some(job) = job else {
            println!("Worker 收到关闭信号，退出");
            break;
        };

        let id = job.id.clone();

        let claimed = {
            let mut retry = RetryDelay::new();

            loop {
                match storage::claim_task(&db, &id).await {
                    Ok(true) => {
                        println!("Worker 已认领任务 {id}");
                        break true;
                    }
                    Ok(false) => {
                        println!("任务 {id} 当前不处于 queued 状态，跳过执行");
                        break false;
                    }
                    Err(error) => {
                        eprintln!("认领任务 {id} 失败：{error}，稍后重试");
                        retry.wait().await;
                    }
                }
            }
        };

        if !claimed {
            continue;
        }

        let execution = tokio::task::spawn_blocking(move || {
            let started = Instant::now();
            let result = execute_job(job);
            let elapsed = started.elapsed();
            (result, elapsed)
        })
        .await;

        match execution {
            Ok((Ok(result), elapsed)) => {
                let duration_ms = duration_to_ms(elapsed);
                persist_succeeded(&db, &id, &result, duration_ms).await;
            }
            Ok((Err(message), elapsed)) => {
                let duration_ms = duration_to_ms(elapsed);
                eprintln!("任务 {id} 执行失败：{message}");
                persist_failed(&db, &id, &message, Some(duration_ms)).await;
            }
            Err(join_error) => {
                let message = format!("回测执行线程异常：{join_error}");
                eprintln!("任务 {id}：{message}");
                persist_failed(&db, &id, &message, None).await;
            }
        }
    }
}
