use crate::{
    backtest::execute_job,
    broker::{BACKTEST_QUEUE, connect_channel},
    storage,
    metrics::Metrics,
    tasks::{ApiBacktestResult, BacktestJob},
};
use futures_util::StreamExt;
use lapin::{
    message::Delivery,
    options::{
        BasicAckOptions, BasicCancelOptions, BasicConsumeOptions, BasicQosOptions,
        BasicRejectOptions,
    },
    types::FieldTable,
};
use sqlx::PgPool;
use std::{
    cmp,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use std::sync::Arc;

const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(100);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);
const BROKER_RETRY_DELAY: Duration = Duration::from_secs(2);

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
    metrics: &Metrics,
) {
    let mut retry = RetryDelay::new();

    loop {
        let persist_started = Instant::now();
        match storage::mark_succeeded(db, id, result, execution_duration_ms).await {
            Ok(()) => {
                metrics.record_duration("result_persist", persist_started.elapsed());
                metrics.increment_tasks_succeeded();
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

async fn persist_failed(db: &PgPool, id: &str, message: &str, execution_duration_ms: Option<i64>, metrics: &Metrics) {
    let mut retry = RetryDelay::new();

    loop {
        let persist_started = Instant::now();
        match storage::mark_failed(db, id, message, execution_duration_ms).await {
            Ok(()) => {
                metrics.record_duration("result_persist", persist_started.elapsed());
                metrics.increment_tasks_failed();
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

/// 一个 Worker 对应一个 RabbitMQ Consumer。
/// prefetch_count=1，确保每个 Worker 同时只持有一条未确认消息。
pub async fn worker_loop(
    worker_id: usize,
    db: PgPool,
    amqp_addr: String,
    mut shutdown: watch::Receiver<bool>,
    metrics: Arc<Metrics>,
) {
    let consumer_tag = format!("backtest-worker-{worker_id}");
    println!("Worker {worker_id} 已启动");

    loop {
        if *shutdown.borrow() {
            break;
        }

        let (connection, channel) = match connect_channel(&amqp_addr).await {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("Worker {worker_id} 连接 RabbitMQ 失败：{error}");
                if wait_or_shutdown(&mut shutdown).await {
                    break;
                }
                continue;
            }
        };

        if let Err(error) = channel.basic_qos(1, BasicQosOptions::default()).await {
            eprintln!("Worker {worker_id} 设置 prefetch 失败：{error}");
            drop(channel);
            drop(connection);

            if wait_or_shutdown(&mut shutdown).await {
                break;
            }
            continue;
        }

        let mut consumer = match channel
            .basic_consume(
                BACKTEST_QUEUE.into(),
                &consumer_tag,
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
        {
            Ok(consumer) => consumer,
            Err(error) => {
                eprintln!("Worker {worker_id} 创建 Consumer 失败：{error}");
                drop(channel);
                drop(connection);

                if wait_or_shutdown(&mut shutdown).await {
                    break;
                }
                continue;
            }
        };

        metrics.set_rabbitmq_connected(true);
        println!("Worker {worker_id} 已连接 RabbitMQ，开始消费任务");

        let mut reconnect = false;

        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    let _ = changed;
                    let _ = channel
                        .basic_cancel(
                            &consumer_tag,
                            BasicCancelOptions::default(),
                        )
                        .await;
                    println!("Worker {worker_id} 收到关闭信号");
                    return;
                }

                delivery = consumer.next() => {
                    match delivery {
                        Some(Ok(delivery)) => {
                            if !process_delivery(&db, delivery, &metrics).await {
                                reconnect = true;
                                break;
                            }
                        }
                        Some(Err(error)) => {
                            eprintln!("Worker {worker_id} 接收消息失败：{error}");
                            reconnect = true;
                            break;
                        }
                        None => {
                            println!("Worker {worker_id} 的 Consumer 已关闭");
                            reconnect = true;
                            break;
                        }
                    }
                }
            }
        }

        metrics.set_rabbitmq_connected(false);
        drop(consumer);
        drop(channel);
        drop(connection);

        if reconnect && wait_or_shutdown(&mut shutdown).await {
            break;
        }
    }

    println!("Worker {worker_id} 已退出");
}

async fn process_delivery(db: &PgPool, delivery: Delivery, metrics: &Metrics) -> bool {
    metrics.increment_consume_total();
    let job: BacktestJob = match serde_json::from_slice(&delivery.data) {
        Ok(job) => job,
        Err(error) => {
            eprintln!("收到无法解析的 RabbitMQ 消息：{error}，拒绝该消息");
            metrics.increment_reject_total();
            return delivery
                .reject(BasicRejectOptions { requeue: false })
                .await
                .is_ok();
        }
    };

    let id = job.id.clone();

    // 数据库状态是最终幂等依据。重复投递时，只有 queued 任务可以被认领。
    let claim_started = Instant::now();
    let claimed = {
        let mut retry = RetryDelay::new();

        loop {
            match storage::claim_task(db, &id).await {
                Ok(true) => {
                    println!("Worker 已认领任务 {id}");
                    break true;
                }
                Ok(false) => {
                    println!("任务 {id} 已被认领或已经结束，跳过重复消息");
                    break false;
                }
                Err(error) => {
                    eprintln!("认领任务 {id} 失败：{error}，稍后重试");
                    retry.wait().await;
                }
            }
        }
    };

    metrics.record_duration("worker_claim", claim_started.elapsed());
    if !claimed {
        metrics.increment_tasks_duplicate();
        return match delivery.ack(BasicAckOptions::default()).await {
            Ok(()) => { metrics.increment_ack_total(); true },
            Err(_) => { metrics.increment_ack_failed(); false },
        };
    }

    metrics.worker_started();
    let execution_metrics = metrics.clone();
    let execution = tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let result = execute_job(job, execution_metrics);
        let elapsed = started.elapsed();
        (result, elapsed)
    })
    .await;

    match execution {
        Ok((Ok(result), elapsed)) => {
            let duration_ms = duration_to_ms(elapsed);
            persist_succeeded(db, &id, &result, duration_ms, metrics).await;
        }
        Ok((Err(message), elapsed)) => {
            let duration_ms = duration_to_ms(elapsed);
            eprintln!("任务 {id} 执行失败：{message}");
            persist_failed(db, &id, &message, Some(duration_ms), metrics).await;
        }
        Err(join_error) => {
            let message = format!("回测执行线程异常：{join_error}");
            eprintln!("任务 {id}：{message}");
            persist_failed(db, &id, &message, None, metrics).await;
        }
    }

    // 只有任务最终状态已经写入 PostgreSQL，才 ACK RabbitMQ 消息。
    metrics.worker_finished();
    let ack_started = Instant::now();
    match delivery.ack(BasicAckOptions::default()).await {
        Ok(()) => {
            metrics.record_duration("rabbitmq_ack", ack_started.elapsed());
            metrics.increment_ack_total();
            println!("任务 {id} 已 ACK");
            true
        }
        Err(error) => {
            metrics.record_duration("rabbitmq_ack", ack_started.elapsed());
            metrics.increment_ack_failed();
            eprintln!("任务 {id} 结果已持久化，但 ACK 失败：{error}");
            false
        }
    }
}

async fn wait_or_shutdown(shutdown: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(BROKER_RETRY_DELAY) => false,
        changed = shutdown.changed() => {
            let _ = changed;
            true
        }
    }
}
