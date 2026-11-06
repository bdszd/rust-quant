# Backend local development

Backend services:
- PostgreSQL: task and result persistence.
- RabbitMQ: durable asynchronous task delivery.
- Transactional Outbox: atomically stores a task and its publishable message in PostgreSQL.
- Outbox Publisher: publishes pending messages and marks them published only after RabbitMQ Publisher Confirm.
- Workers: consume RabbitMQ messages, claim tasks in PostgreSQL, execute Backtest Core, persist the final state, then ACK.

## 1. Start infrastructure

From the repository root:

\`\`\`bash
docker compose up -d postgres rabbitmq
docker compose ps
\`\`\`

PostgreSQL is exposed on host port 5433. RabbitMQ AMQP is exposed on 5673 and its management UI on 15673.

Management UI: http://localhost:15673
Development login: rust_quant / rust_quant_dev

These credentials are for local development only.

## 2. Configure the backend

\`\`\`bash
export DATABASE_URL='postgres://postgres:postgres@localhost:5433/rust_quant'
export AMQP_ADDR='amqp://rust_quant:rust_quant_dev@localhost:5673/%2f'
\`\`\`

The application reads environment variables directly; it does not automatically load a .env file.

Optional data directory override:

\`\`\`bash
export RUST_QUANT_DATA_DIR="$PWD/data/cleaned"
\`\`\`

By default, the backend reads cleaned datasets from the repository's data/cleaned directory.

## 3. Cleaned dataset layout

Each dataset uses this layout:

\`\`\`text
data/cleaned/
└── 000001.SZ/
    └── 20240101_20241231/
        ├── bars.csv
        ├── rejected.csv
        └── quality_report.json
\`\`\`

The API dataset identifier is \`000001.SZ/20240101_20241231\`. The API reads \`accepted_rows\` from \`quality_report.json\`; it does not load the CSV to calculate row counts.

## 4. Run the backend

\`\`\`bash
cargo run -p quant-backend
\`\`\`

SQLx applies pending migrations at startup.

## 5. Task delivery flow

1. Axum validates the request and dataset.
2. PostgreSQL transaction inserts \`backtest_tasks\` and \`backtest_outbox\`.
3. Outbox Publisher publishes the payload to the durable RabbitMQ queue.
4. Publisher Confirm allows the Outbox row to be marked published.
5. Worker consumes the message and atomically changes the task from queued to running.
6. Worker executes Backtest Core and persists succeeded/failed state.
7. Worker ACKs the RabbitMQ delivery only after the final state is persisted.

Delivery is at-least-once, not exactly-once. A message can be delivered again if the process fails after database persistence but before ACK. The task ID and conditional database claim make duplicate deliveries harmless.

At startup, tasks left in running state are reset to queued and their Outbox events are made pending again. This is a single-backend-instance recovery strategy; multi-instance deployments need leases/heartbeats rather than resetting every running task.

## 6. Existing PostgreSQL data

The new migration creates the Outbox table without deleting existing task or result rows. Existing queued tasks are backfilled into the Outbox when the backend starts. Running tasks from the previous in-memory dispatcher are reset and re-enqueued.
