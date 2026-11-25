use crate::{
    infrastructure::messaging::rabbitmq::{BACKTEST_QUEUE, connect_channel},
    modules::backtest::infrastructure::postgres_repository::PgBacktestRepository,
    modules::backtest::service::execute_config,
    observability::metrics::Metrics,
};
use futures_util::StreamExt;
use lapin::{
    message::Delivery,
    options::{
        BasicAckOptions, BasicCancelOptions, BasicConsumeOptions, BasicNackOptions,
        BasicQosOptions, BasicRejectOptions,
    },
    types::FieldTable,
};
use serde::Deserialize;
use std::{
    cmp,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;

const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(100);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);
const BROKER_RETRY_DELAY: Duration = Duration::from_secs(2);
const EXECUTION_LEASE_SECONDS: i64 = 60;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
struct BacktestRequestedMessage {
    event_id: Uuid,
    task_id: Uuid,
    job_id: Uuid,
}

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

/// 一个 Worker 对应一个 RabbitMQ Consumer。
/// 消息只携带 task/job 标识，真正的执行配置从 PostgreSQL 读取。
pub async fn worker_loop(
    worker_id: usize,
    repository: Arc<PgBacktestRepository>,
    data_dir: PathBuf,
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
                BACKTEST_QUEUE,
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

        metrics.rabbitmq_connection_opened();
        println!("Worker {worker_id} 已连接 RabbitMQ，开始消费任务");

        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    let _ = changed;
                    let _ = channel
                        .basic_cancel(&consumer_tag, BasicCancelOptions::default())
                        .await;
                    metrics.rabbitmq_connection_closed();
                    return;
                }
                delivery = consumer.next() => {
                    match delivery {
                        Some(Ok(delivery)) => {
                            if !process_delivery(
                                &repository,
                                &data_dir,
                                delivery,
                                worker_id,
                                metrics.clone(),
                            ).await {
                                break;
                            }
                        }
                        Some(Err(error)) => {
                            eprintln!("Worker {worker_id} 接收消息失败：{error}");
                            break;
                        }
                        None => {
                            println!("Worker {worker_id} 的 Consumer 已关闭");
                            break;
                        }
                    }
                }
            }
        }

        metrics.rabbitmq_connection_closed();
        drop(consumer);
        drop(channel);
        drop(connection);

        if wait_or_shutdown(&mut shutdown).await {
            break;
        }
    }

    println!("Worker {worker_id} 已退出");
}

async fn process_delivery(
    repository: &Arc<PgBacktestRepository>,
    data_dir: &Path,
    delivery: Delivery,
    worker_number: usize,
    metrics: Arc<Metrics>,
) -> bool {
    metrics.increment_consume_total();

    let message: BacktestRequestedMessage = match serde_json::from_slice(&delivery.data) {
        Ok(message) => message,
        Err(error) => {
            eprintln!("收到格式无效的回测消息，拒绝并交给 DLQ：{error}");
            metrics.increment_reject_total();
            return delivery
                .reject(BasicRejectOptions { requeue: false })
                .await
                .is_ok();
        }
    };

    let worker_id = format!("worker-{worker_number}-{}", Uuid::new_v4());
    let claim_started = Instant::now();

    let claimed = match repository
        .claim_execution(
            message.task_id,
            message.job_id,
            &worker_id,
            EXECUTION_LEASE_SECONDS,
        )
        .await
    {
        Ok(claimed) => claimed,
        Err(error) => {
            eprintln!(
                "认领任务 {}（事件 {}）失败：{error}，消息重新入队",
                message.task_id, message.event_id
            );
            return delivery
                .nack(BasicNackOptions {
                    requeue: true,
                    ..BasicNackOptions::default()
                })
                .await
                .is_ok();
        }
    };

    metrics.record_duration("worker_claim", claim_started.elapsed());

    let Some(claimed) = claimed else {
        // 任务已结束，或另一个 Worker 持有未过期租约。重复消息可安全 ACK。
        metrics.increment_tasks_duplicate();
        return match delivery.ack(BasicAckOptions::default()).await {
            Ok(()) => {
                metrics.increment_ack_total();
                true
            }
            Err(_) => {
                metrics.increment_ack_failed();
                false
            }
        };
    };

    println!(
        "Worker 已认领任务 {}，Attempt {}",
        claimed.task_id, claimed.attempt_number
    );
    metrics.worker_started();

    // 执行期间定时续租，避免长回测被其他 Worker 当成过期任务接管。
    let (heartbeat_stop_tx, mut heartbeat_stop_rx) = watch::channel(false);
    let heartbeat_repository = repository.clone();
    let heartbeat_attempt_id = claimed.attempt_id;
    let heartbeat_worker_id = worker_id.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        interval.tick().await;
        loop {
            tokio::select! {
                changed = heartbeat_stop_rx.changed() => {
                    let _ = changed;
                    break;
                }
                _ = interval.tick() => {
                    match heartbeat_repository
                        .heartbeat_attempt(
                            heartbeat_attempt_id,
                            &heartbeat_worker_id,
                            EXECUTION_LEASE_SECONDS,
                        )
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            eprintln!("Attempt {heartbeat_attempt_id} 的租约所有权已失效");
                            break;
                        }
                        Err(error) => {
                            eprintln!("Attempt {heartbeat_attempt_id} 续租失败：{error}");
                        }
                    }
                }
            }
        }
    });

    let task_id = claimed.task_id;
    let attempt_id = claimed.attempt_id;
    let config = claimed.config;
    let execution_data_dir = data_dir.clone();
    let execution_metrics = metrics.clone();
    let execution_started = Instant::now();

    let execution = tokio::task::spawn_blocking(move || {
        execute_config(task_id, config, &execution_data_dir, execution_metrics)
    })
    .await;

    let execution_duration_ms = duration_to_ms(execution_started.elapsed());
    let persistence_result = match execution {
        Ok(Ok(result)) => match serde_json::to_value(&result) {
            Ok(payload) => {
                persist_success(
                    repository,
                    attempt_id,
                    task_id,
                    &worker_id,
                    &payload,
                    execution_duration_ms,
                )
                .await
            }
            Err(error) => {
                let message = format!("序列化回测结果失败：{error}");
                persist_failure(
                    repository,
                    attempt_id,
                    task_id,
                    &worker_id,
                    &message,
                    Some(execution_duration_ms),
                )
                .await
            }
        },
        Ok(Err(message)) => {
            eprintln!("任务 {task_id} 执行失败：{message}");
            persist_failure(
                repository,
                attempt_id,
                task_id,
                &worker_id,
                &message,
                Some(execution_duration_ms),
            )
            .await
        }
        Err(join_error) => {
            let message = format!("回测执行线程异常：{join_error}");
            persist_failure(
                repository,
                attempt_id,
                task_id,
                &worker_id,
                &message,
                Some(execution_duration_ms),
            )
            .await
        }
    };

    // 持久化重试期间继续续租，避免数据库短暂不可用时被其他 Worker 抢占。
    let _ = heartbeat_stop_tx.send(true);
    let _ = heartbeat_handle.await;

    if let Err(error) = persistence_result {
        eprintln!("任务 {task_id} 的最终状态未能持久化：{error}，消息重新入队");
        metrics.worker_finished();
        return delivery
            .nack(BasicNackOptions {
                requeue: true,
                ..BasicNackOptions::default()
            })
            .await
            .is_ok();
    }

    metrics.worker_finished();
    let ack_started = Instant::now();
    match delivery.ack(BasicAckOptions::default()).await {
        Ok(()) => {
            metrics.record_duration("rabbitmq_ack", ack_started.elapsed());
            metrics.increment_ack_total();
            println!("任务 {task_id} 的 Attempt 已持久化，消息已 ACK");
            true
        }
        Err(error) => {
            metrics.record_duration("rabbitmq_ack", ack_started.elapsed());
            metrics.increment_ack_failed();
            eprintln!("任务 {task_id} 已持久化，但 ACK 失败：{error}");
            false
        }
    }
}

async fn persist_success(
    repository: &PgBacktestRepository,
    attempt_id: Uuid,
    task_id: Uuid,
    worker_id: &str,
    payload: &serde_json::Value,
    duration_ms: i64,
) -> Result<(), String> {
    let mut retry = RetryDelay::new();
    loop {
        match repository
            .complete_success(attempt_id, task_id, worker_id, payload, duration_ms)
            .await
        {
            Ok(()) => return Ok(()),
            Err(error) if error.contains("不再拥有该执行 Attempt") => {
                return Err(error);
            }
            Err(error) => {
                eprintln!("保存任务 {task_id} 成功结果失败：{error}，稍后重试");
                retry.wait().await;
            }
        }
    }
}

async fn persist_failure(
    repository: &PgBacktestRepository,
    attempt_id: Uuid,
    task_id: Uuid,
    worker_id: &str,
    message: &str,
    duration_ms: Option<i64>,
) -> Result<(), String> {
    let mut retry = RetryDelay::new();
    loop {
        match repository
            .complete_failure(attempt_id, task_id, worker_id, message, duration_ms)
            .await
        {
            Ok(()) => return Ok(()),
            Err(error) if error.contains("不再拥有该执行 Attempt") => {
                return Err(error);
            }
            Err(error) => {
                eprintln!("保存任务 {task_id} 失败状态失败：{error}，稍后重试");
                retry.wait().await;
            }
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
