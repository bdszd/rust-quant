use crate::{
    backtest::execute_job,
    storage,
    tasks::{ApiBacktestResult, BacktestJob},
};
use sqlx::SqlitePool;
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

/// 将 Duration 转换为毫秒。
fn duration_to_ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// 持续重试，直到任务成功写入数据库。
async fn persist_succeeded(
    db: &SqlitePool,
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

/// 持续重试，直到任务失败状态写入数据库。
async fn persist_failed(
    db: &SqlitePool,
    id: &str,
    message: &str,
    execution_duration_ms: Option<i64>,
) {
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

pub async fn worker_loop(receiver: Arc<AsyncMutex<mpsc::Receiver<BacktestJob>>>, db: SqlitePool) {
    loop {
        // 只在接收任务时持有 Receiver 锁。
        // 收到任务后立即释放锁，不影响其他 Worker 接收任务。
        let job = {
            let mut receiver = receiver.lock().await;
            receiver.recv().await
        };

        let Some(job) = job else {
            println!("Worker 收到关闭信号，退出");
            break;
        };

        let id = job.id.clone();

        // 第一步：向数据库原子认领任务。
        //
        // 数据库暂时不可用时，保留当前 job 并持续重试，
        // 不允许直接丢弃已经从内存队列取出的任务。
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

        // 第二步：执行回测并测量实际执行耗时。
        //
        // 计时在 blocking 线程真正开始执行时启动，
        // 不包括任务在线程池中的排队等待时间。
        let execution = tokio::task::spawn_blocking(move || {
            let started = Instant::now();

            let result = execute_job(job);

            let elapsed = started.elapsed();

            (result, elapsed)
        })
        .await;

        // 第三步：将执行结果和耗时持久化。
        //
        // spawn_blocking().await 的返回类型：
        // Result<(Result<ApiBacktestResult, String>, Duration), JoinError>
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
