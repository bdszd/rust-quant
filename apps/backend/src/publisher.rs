use crate::{
    broker::{BACKTEST_QUEUE, connect_channel},
    metrics::Metrics,
    storage,
};
use lapin::{
    BasicProperties,
    options::{BasicPublishOptions, ConfirmSelectOptions},
};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const PUBLISH_INTERVAL: Duration = Duration::from_millis(500);
const RETRY_DELAY: Duration = Duration::from_secs(2);
const BATCH_SIZE: i64 = 32;

/// 从 PostgreSQL Outbox 读取待发布任务，并投递到 RabbitMQ。
///
/// 只有收到 RabbitMQ Publisher Confirm 后，才将 Outbox 记录标记为已发布。
pub async fn publisher_loop(
    db: PgPool,
    amqp_addr: String,
    mut shutdown: watch::Receiver<bool>,
    metrics: Arc<Metrics>,
) {
    println!("Outbox Publisher 已启动");

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
        println!("Outbox Publisher 已连接 RabbitMQ");

        let mut reconnect = false;

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    println!("Outbox Publisher 收到关闭信号");
                    metrics.rabbitmq_connection_closed();
                    return;
                }

                _ = tokio::time::sleep(PUBLISH_INTERVAL) => {
                    let query_started = std::time::Instant::now();

                    let messages = match storage::list_pending_outbox(&db, BATCH_SIZE).await {
                        Ok(messages) => messages,
                        Err(error) => {
                            eprintln!("读取 Outbox 失败：{error}");
                            continue;
                        }
                    };

                    metrics.record_duration("outbox_query", query_started.elapsed());

                    // 收集本批次中已经收到 RabbitMQ Confirm 的任务。
                    let mut confirmed_task_ids = Vec::with_capacity(messages.len());

                    for message in messages {
                        let publish_started = std::time::Instant::now();

                        metrics.increment_publish_total();

                        if let Err(error) = publish_one(&channel, &message.payload).await {
                            metrics.increment_publish_failed();

                            if error.contains("NACK") || error.contains("Confirm") {
                                metrics.increment_publish_confirm_failed();
                            }

                            metrics.record_duration(
                                "rabbitmq_publish_confirm",
                                publish_started.elapsed()
                            );

                            eprintln!(
                                "发布任务 {} 到 RabbitMQ 失败：{error}，准备重新连接",
                                message.task_id
                            );

                            reconnect = true;
                            break;
                        }

                        metrics.record_duration(
                            "rabbitmq_publish_confirm",
                            publish_started.elapsed()
                        );

                        // 这里只收集 ID，暂时不逐条 UPDATE 数据库。
                        confirmed_task_ids.push(message.task_id);
                    }

                    // 一批消息发布完成后，用一条 SQL 批量更新 Outbox。
                    //
                    // 即使本批次中途发布失败，也要先标记已经 Confirm 的消息，
                    // 避免不必要的重复投递。
                    if !confirmed_task_ids.is_empty() {
                        let update_started = std::time::Instant::now();

                        match storage::mark_outbox_published_batch(
                            &db,
                            &confirmed_task_ids
                        ).await {
                            Ok(updated) => {
                                println!(
                                    "本批次 {} 条消息已收到 Confirm，成功标记 {} 条 Outbox 记录",
                                    confirmed_task_ids.len(),
                                    updated
                                );
                            }
                            Err(error) => {
                                // RabbitMQ 已确认但数据库未标记时，
                                // 下一轮可能重复发布。
                                // Worker 仍通过 task_id 和数据库状态保证幂等。
                                eprintln!(
                                    "批量更新 Outbox 状态失败：{error}，后续可能重复发布"
                                );
                            }
                        }

                        metrics.record_duration(
                            "outbox_update",
                            update_started.elapsed()
                        );
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

    println!("Outbox Publisher 已退出");
}

async fn publish_one(channel: &lapin::Channel, payload: &str) -> Result<(), String> {
    let confirm = channel
        .basic_publish(
            "",
            BACKTEST_QUEUE,
            BasicPublishOptions {
                mandatory: true,
                ..BasicPublishOptions::default()
            },
            payload.as_bytes(),
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
