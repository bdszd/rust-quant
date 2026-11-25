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

By default, the backend reads cleaned datasets from `data/cleaned/tushare`. The directory can be overridden with `RUST_QUANT_DATA_DIR`.

## 3. Cleaned dataset layout

Each dataset uses this layout:

\`\`\`text
data/cleaned/tushare/
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

## 5. Per-instance runtime settings

Each Backend process has its own PostgreSQL connection pool, Publisher, HTTP listener, and configurable number of RabbitMQ Workers.

| Environment variable | Default | Purpose |
| --- | ---: | --- |
| `RUST_QUANT_INSTANCE_ID` | generated UUID | Identifies this Backend instance in logs and Worker ownership IDs. Set a unique stable value per deployed instance. |
| `RUST_QUANT_WORKER_COUNT` | `2` | Number of RabbitMQ consumers started by this instance. Set to `0` for an API/Publisher-only instance. |
| `RUST_QUANT_DB_MAX_CONNECTIONS` | `10` | Maximum PostgreSQL connections in this instance's pool. |
| `RUST_QUANT_RUN_MIGRATIONS` | `true` | Whether this process applies pending SQLx migrations at startup. |
| `RUST_QUANT_MIGRATE_ONLY` | `false` | Apply migrations and exit without starting HTTP, Publisher, or Workers. |

For example, run the migration step once before deploying or scaling the serving replicas:

```bash
DATABASE_URL='postgres://...' RUST_QUANT_MIGRATE_ONLY=true cargo run -p quant-backend
```

Then start every serving instance with migrations disabled and a unique instance ID:

```bash
RUST_QUANT_RUN_MIGRATIONS=false
RUST_QUANT_INSTANCE_ID=backend-1
RUST_QUANT_WORKER_COUNT=2
RUST_QUANT_DB_MAX_CONNECTIONS=10
```

The second instance uses a different `RUST_QUANT_INSTANCE_ID`. Both instances connect to the same PostgreSQL database and RabbitMQ queue. RabbitMQ distributes messages among their consumers. The total PostgreSQL connection budget must account for all instances (for example, 3 instances × 10 connections = up to 30 connections).

These settings prepare the process and task execution layer for multiple instances. They do not yet make the cleaned CSV dataset directory shared: all instances must currently see equivalent dataset files at the configured path. Shared dataset storage and the full multi-instance data flow are intentionally deferred.

## 7. Task delivery flow

1. Axum validates the request and dataset.
2. PostgreSQL transaction inserts \`backtest_tasks\` and \`backtest_outbox\`.
3. Outbox Publisher publishes the payload to the durable RabbitMQ queue.
4. Publisher Confirm allows the Outbox row to be marked published.
5. Worker consumes the message and atomically claims the task and creates an execution Attempt.
6. Worker periodically renews the Attempt lease while Backtest Core is running.
7. The executor loads the cleaned CSV dataset, validates its bars, and runs the selected strategy.
8. Worker persists the result (or failure), Attempt status, and Task status in a database transaction.
9. Worker ACKs the RabbitMQ delivery only after the final state is persisted.

Delivery is at-least-once, not exactly-once. A message can be delivered again if the process fails after database persistence but before ACK. The task ID and conditional database claim make duplicate deliveries harmless.

A task left in `running` is not reset during process startup. If its execution lease expires, a redelivered message can mark the old Attempt interrupted and create a new Attempt. The heartbeat prevents a healthy long-running execution from being taken over by another Worker.

## 8. Migration ownership

The Backend is the active SQLx migration owner for its identity, session, task, job, Attempt, result, and Outbox tables. Startup runs only the migrations under `apps/backend/migrations`.

The SQL file under `crates/market-data/migrations` is currently not invoked by the Market Data crate. Do not add a second migrator using SQLx's default migration table. If Market Data persistence is activated later, its schema migration must be coordinated with the Backend migration owner or use an explicitly separate migration table/schema.

## 9. Existing PostgreSQL data

The backtest migration creates the normalized task, job, Attempt, result, and Outbox tables. If it finds incompatible legacy `backtest_tasks`, `backtest_jobs`, or `backtest_outbox` tables, it renames them to `*_legacy_v1` and leaves their data intact; it does not attempt an automatic data conversion. It does not delete existing data or Docker volumes. No migration is run as part of this refactor work. Use the migration-only mode or the normal local-development startup to apply migrations explicitly.
