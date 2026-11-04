use crate::{
    storage,
    tasks::BacktestJob,
};

use sqlx::SqlitePool;
use std::{
    collections::HashSet,
    time::Duration,
};

use tokio::{
    sync::{mpsc, oneshot},
    time::interval,
};

const DISPATCH_INTERVAL: Duration = Duration::from_millis(500);

pub async fn dispatcher_loop(
    db: SqlitePool,
    sender: mpsc::Sender<BacktestJob>,
    mut shutdown: oneshot::Receiver<()>,
) {
    println!("Dispatcher 已启动");

    let mut ticker = interval(DISPATCH_INTERVAL);

    // 记录已经投递到 mpsc、但数据库状态仍然是 queued 的任务。
    let mut dispatched = HashSet::<String>::new();

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                println!("Dispatcher 收到关闭信号");
                break;
            }

            _ = ticker.tick() => {
                // 1. 从数据库读取所有 queued 任务。
                let jobs = match storage::list_queued_jobs(&db).await {
                    Ok(jobs) => jobs,

                    Err(error) => {
                        eprintln!(
                            "Dispatcher 查询任务失败：{error}"
                        );
                        continue;
                    }
                };

                // 2. 获取当前数据库中仍处于 queued 状态的任务 ID。
                let queued_ids: HashSet<String> = jobs
                    .iter()
                    .map(|job| job.id.clone())
                    .collect();

                // 3. 清除已经被 Worker 认领的任务 ID。
                dispatched.retain(|id| queued_ids.contains(id));

                // 4. 将尚未投递的任务发送到 mpsc。
                for job in jobs {
                    let id = job.id.clone();

                    // 已经投递过，而且仍处于 queued 状态。
                    // 说明它可能还在队列里等待 Worker 消费。
                    if dispatched.contains(&id) {
                        continue;
                    }

                    match sender.try_send(job) {
                        Ok(()) => {
                            println!("Dispatcher 投递任务 {id}");

                            dispatched.insert(id);
                        }

                        Err(mpsc::error::TrySendError::Full(_job)) => {
                            // 队列满了，不继续投递。
                            // 下次轮询时重新从数据库读取。
                            break;
                        }

                        Err(mpsc::error::TrySendError::Closed(_job)) => {
                            println!("任务队列已关闭，Dispatcher 退出");
                            return;
                        }
                    }
                }
            }
        }
    }

    println!("Dispatcher 已退出");
}