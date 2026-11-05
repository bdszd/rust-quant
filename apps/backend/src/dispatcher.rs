use crate::{storage, tasks::BacktestJob};

use sqlx::PgPool;
use std::{collections::HashSet, time::Duration};

use tokio::{
    sync::{mpsc, oneshot},
    time::interval,
};

const DISPATCH_INTERVAL: Duration = Duration::from_millis(500);

pub async fn dispatcher_loop(
    db: PgPool,
    sender: mpsc::Sender<BacktestJob>,
    mut shutdown: oneshot::Receiver<()>,
) {
    println!("Dispatcher 已启动");

    let mut ticker = interval(DISPATCH_INTERVAL);
    let mut dispatched = HashSet::<String>::new();

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                println!("Dispatcher 收到关闭信号");
                break;
            }

            _ = ticker.tick() => {
                let jobs = match storage::list_queued_jobs(&db).await {
                    Ok(jobs) => jobs,
                    Err(error) => {
                        eprintln!("Dispatcher 查询任务失败：{error}");
                        continue;
                    }
                };

                let queued_ids: HashSet<String> = jobs
                    .iter()
                    .map(|job| job.id.clone())
                    .collect();

                dispatched.retain(|id| queued_ids.contains(id));

                for job in jobs {
                    let id = job.id.clone();

                    if dispatched.contains(&id) {
                        continue;
                    }

                    match sender.try_send(job) {
                        Ok(()) => {
                            println!("Dispatcher 投递任务 {id}");
                            dispatched.insert(id);
                        }
                        Err(mpsc::error::TrySendError::Full(_job)) => break,
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
