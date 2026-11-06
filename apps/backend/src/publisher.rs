use crate::{
    broker::{BACKTEST_QUEUE, connect_channel},
    storage,
};
use lapin::{
    BasicProperties,
    options::{BasicPublishOptions, ConfirmSelectOptions},
};
use sqlx::PgPool;
use std::time::Duration;
use tokio::sync::watch;

const PUBLISH_INTERVAL: Duration = Duration::from_millis(500);
const RETRY_DELAY: Duration = Duration::from_secs(2);
const BATCH_SIZE: i64 = 32;

/// 从 PostgreSQL Outbox 读取待发布任务，并投递到 RabbitMQ。
///
/// 只有收到 RabbitMQ Publisher Confirm 后，才将 Outbox 记录标记为已发布。
pub async fn publisher_loop(db: PgPool, amqp_addr: String, mut shutdown: watch::Receiver<bool>) {
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

        println!("Outbox Publisher 已连接 RabbitMQ");

        let mut reconnect = false;

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    println!("Outbox Publisher 收到关闭信号");
                    return;
                }

                _ = tokio::time::sleep(PUBLISH_INTERVAL) => {
                    let messages = match storage::list_pending_outbox(&db, BATCH_SIZE).await {
                        Ok(messages) => messages,
                        Err(error) => {
                            eprintln!("读取 Outbox 失败：{error}");
                            continue;
                        }
                    };

                    for message in messages {
                        if let Err(error) = publish_one(&channel, &message.payload).await {
                            eprintln!(
                                "发布任务 {} 到 RabbitMQ 失败：{error}，准备重新连接",
                                message.task_id
                            );
                            reconnect = true;
                            break;
                        }

                        if let Err(error) =
                            storage::mark_outbox_published(&db, &message.task_id).await
                        {
                            // Broker 已确认但数据库未标记时，下一轮可能重复发布。
                            // Worker 通过 task_id 和数据库状态保证幂等。
                            eprintln!(
                                "任务 {} 已被 RabbitMQ 确认，但更新 Outbox 状态失败：{error}",
                                message.task_id
                            );
                        } else {
                            println!("任务 {} 已发布到 RabbitMQ", message.task_id);
                        }
                    }

                    if reconnect {
                        break;
                    }
                }
            }
        }

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
            "".into(),
            BACKTEST_QUEUE.into(),
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
