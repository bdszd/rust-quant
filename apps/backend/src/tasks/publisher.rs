use crate::{
    infrastructure::messaging::rabbitmq::{BACKTEST_QUEUE, connect_channel},
    modules::backtest::{
        application::ports::OutboxRepository,
    },
    observability::metrics::Metrics,
};
use lapin::{
    BasicProperties,
    options::{BasicPublishOptions, ConfirmSelectOptions},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

const PUBLISH_INTERVAL: Duration = Duration::from_millis(500);
const RETRY_DELAY: Duration = Duration::from_secs(2);
const BATCH_SIZE: i64 = 32;
const OUTBOX_LEASE_SECONDS: i64 = 300;

/// 使用 Outbox Repository 领取事件并发布到 RabbitMQ。
///
/// 数据库领取锁保证多个 Publisher 不会同时正常处理同一条事件。
/// RabbitMQ Confirm 成功后才标记 published；崩溃窗口仍可能产生重复消息，
/// 因此 Worker 必须按 task 状态幂等处理。
pub async fn publisher_loop(
    repository: Arc<dyn OutboxRepository>,
    amqp_addr: String,
    mut shutdown: watch::Receiver<bool>,
    metrics: Arc<Metrics>,
) {
    let publisher_id = Uuid::new_v4().to_string();
    println!("Outbox Publisher {publisher_id} 已启动");

    loop {
        if *shutdown.borrow() {
            break;
        }

        let (connection, channel) = match connect_channel(&amqp_addr).await {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("连接 RabbitMQ 失败：{error}，稍后重试");
                if wait_or_shutdown(&mut shutdown, RETRY_DELAY).await {
                    break;
                }
                continue;
            }
        };

        if let Err(error) = channel
            .confirm_select(ConfirmSelectOptions::default())
            .await
        {
            eprintln!("启用 RabbitMQ Publisher Confirm 失败：{error}");
            drop(channel);
            drop(connection);
            if wait_or_shutdown(&mut shutdown, RETRY_DELAY).await {
                break;
            }
            continue;
        }

        metrics.rabbitmq_connection_opened();
        let mut reconnect = false;

        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    let _ = changed;
                    metrics.rabbitmq_connection_closed();
                    return;
                }
                _ = tokio::time::sleep(PUBLISH_INTERVAL) => {
                    let query_started = std::time::Instant::now();
                    let events = match repository
                        .claim_pending(&publisher_id, BATCH_SIZE, OUTBOX_LEASE_SECONDS)
                        .await
                    {
                        Ok(events) => events,
                        Err(error) => {
                            eprintln!("领取 Outbox 事件失败：{error}");
                            continue;
                        }
                    };
                    metrics.record_duration("outbox_query", query_started.elapsed());

                    for event in events {
                        println!("发布 Outbox 事件类型：{}", event.event_type);
                        metrics.increment_publish_total();
                        let publish_started = std::time::Instant::now();
                        let payload = match serde_json::to_vec(&event.payload) {
                            Ok(payload) => payload,
                            Err(error) => {
                                metrics.increment_publish_failed();
                                let message = format!("序列化 Outbox 事件失败：{error}");
                                let _ = repository
                                    .release_after_failure(event.id, &publisher_id, &message)
                                    .await;
                                continue;
                            }
                        };

                        match publish_one(&channel, &payload).await {
                            Ok(()) => {
                                metrics.record_duration(
                                    "rabbitmq_publish_confirm",
                                    publish_started.elapsed(),
                                );
                                match repository.mark_published(event.id, &publisher_id).await {
                                    Ok(true) => {
                                        println!(
                                            "Outbox 事件 {}（任务 {}）已发布",
                                            event.id, event.task_id
                                        );
                                    }
                                    Ok(false) => {
                                        eprintln!(
                                            "Outbox 事件 {} 的领取锁已失效，未标记 published",
                                            event.id
                                        );
                                    }
                                    Err(error) => {
                                        // Broker 已确认但 DB 未标记时允许重复投递。
                                        eprintln!(
                                            "事件 {} 已被 RabbitMQ 确认，但更新 Outbox 失败：{error}",
                                            event.id
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                metrics.increment_publish_failed();
                                if error.contains("NACK") || error.contains("Confirm") {
                                    metrics.increment_publish_confirm_failed();
                                }
                                metrics.record_duration(
                                    "rabbitmq_publish_confirm",
                                    publish_started.elapsed(),
                                );
                                let _ = repository
                                    .release_after_failure(event.id, &publisher_id, &error)
                                    .await;
                                eprintln!("发布 Outbox 事件 {} 失败：{error}", event.id);
                                reconnect = true;
                                break;
                            }
                        }
                    }

                    if reconnect {
                        break;
                    }
                }
            }
        }

        metrics.rabbitmq_connection_closed();
        drop(channel);
        drop(connection);

        if reconnect && wait_or_shutdown(&mut shutdown, RETRY_DELAY).await {
            break;
        }
    }

    println!("Outbox Publisher {publisher_id} 已退出");
}

async fn publish_one(channel: &lapin::Channel, payload: &[u8]) -> Result<(), String> {
    let confirm = channel
        .basic_publish(
            "",
            BACKTEST_QUEUE,
            BasicPublishOptions {
                mandatory: true,
                ..BasicPublishOptions::default()
            },
            payload,
            BasicProperties::default()
                .with_delivery_mode(2)
                .with_content_type("application/json".into()),
        )
        .await
        .map_err(|error| format!("发送消息失败：{error}"))?;

    let confirmation = confirm
        .await
        .map_err(|error| format!("等待 Publisher Confirm 失败：{error}"))?;

    if !confirmation.is_ack() {
        return Err("RabbitMQ 返回 NACK".to_string());
    }
    if confirmation.take_message().is_some() {
        return Err("消息未能路由到任务队列".to_string());
    }

    Ok(())
}

async fn wait_or_shutdown(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        changed = shutdown.changed() => {
            let _ = changed;
            true
        }
    }
}
