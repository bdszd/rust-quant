use lapin::{
    Channel, Connection, ConnectionProperties, options::QueueDeclareOptions, types::FieldTable,
};

pub const BACKTEST_QUEUE: &str = "backtest.jobs";

/// 建立 RabbitMQ 连接、创建 Channel，并确保持久化任务队列存在。
pub async fn connect_channel(amqp_addr: &str) -> Result<(Connection, Channel), lapin::Error> {
    let connection = Connection::connect(amqp_addr, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;

    channel
        .queue_declare(
            BACKTEST_QUEUE.into(),
            QueueDeclareOptions {
                durable: true,
                ..QueueDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await?;

    Ok((connection, channel))
}
