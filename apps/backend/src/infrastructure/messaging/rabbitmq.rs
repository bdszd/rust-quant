use lapin::{
    Channel, Connection, ConnectionProperties, ExchangeKind,
    options::{ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions},
    types::{AMQPValue, FieldTable},
};

/// 新版任务消息队列。使用独立队列名，避免与旧版完整 Job JSON 消息格式冲突。
pub const BACKTEST_QUEUE: &str = "backtest.jobs.v2";
const BACKTEST_DEAD_LETTER_EXCHANGE: &str = "backtest.dlx";
const BACKTEST_DEAD_LETTER_QUEUE: &str = "backtest.jobs.dlq";
const BACKTEST_DEAD_LETTER_ROUTING_KEY: &str = "backtest.jobs.dead";

/// 建立 RabbitMQ 连接、创建 Channel，并声明任务队列与死信队列。
pub async fn connect_channel(amqp_addr: &str) -> Result<(Connection, Channel), lapin::Error> {
    let connection = Connection::connect(amqp_addr, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;

    channel
        .exchange_declare(
            BACKTEST_DEAD_LETTER_EXCHANGE,
            ExchangeKind::Direct,
            ExchangeDeclareOptions {
                durable: true,
                ..ExchangeDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await?;

    channel
        .queue_declare(
            BACKTEST_DEAD_LETTER_QUEUE,
            QueueDeclareOptions {
                durable: true,
                ..QueueDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await?;

    channel
        .queue_bind(
            BACKTEST_DEAD_LETTER_QUEUE,
            BACKTEST_DEAD_LETTER_EXCHANGE,
            BACKTEST_DEAD_LETTER_ROUTING_KEY,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut queue_arguments = FieldTable::default();
    queue_arguments.insert(
        "x-dead-letter-exchange".into(),
        AMQPValue::LongString(BACKTEST_DEAD_LETTER_EXCHANGE.into()),
    );
    queue_arguments.insert(
        "x-dead-letter-routing-key".into(),
        AMQPValue::LongString(BACKTEST_DEAD_LETTER_ROUTING_KEY.into()),
    );

    channel
        .queue_declare(
            BACKTEST_QUEUE,
            QueueDeclareOptions {
                durable: true,
                ..QueueDeclareOptions::default()
            },
            queue_arguments,
        )
        .await?;

    Ok((connection, channel))
}
