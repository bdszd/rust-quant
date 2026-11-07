# Backend Metrics

## Endpoint

The backend exposes a process-local snapshot:

`GET /api/metrics`

Metrics are reset when the backend process restarts. They are intended for one test run / one backend instance, not as durable historical telemetry.

## What is measured

### Counters

- `tasks.accepted`: backtest requests successfully committed to PostgreSQL.
- `tasks.succeeded` / `tasks.failed`: tasks whose final state was successfully persisted by a worker during this process lifetime.
- `tasks.duplicate`: deliveries that could not claim a task because it was no longer queued.
- `messaging.publish_total`: publish attempts, including attempts that fail.
- `messaging.publish_failed` / `publish_confirm_failed`: publish failures and the subset classified as NACK / publisher-confirm failures.
- `messaging.consume_total`: deliveries received by workers.
- `messaging.ack_total` / `ack_failed`: successful and failed ACK attempts.
- `messaging.reject_total`: malformed JSON deliveries rejected without requeue.

### Stage latency

Each populated stage reports `count`, `avg_ms`, `min_ms`, `max_ms`, `p50_ms`, `p95_ms`, and `p99_ms`.

| Stage | Measurement |
| --- | --- |
| `db_insert_task` | PostgreSQL transaction inserting task + Outbox |
| `outbox_query` | Query for pending Outbox messages |
| `rabbitmq_publish_confirm` | Publish and wait for RabbitMQ Publisher Confirm |
| `outbox_update` | Marking an Outbox row as published |
| `worker_claim` | Worker claim attempt, including retry backoff on DB errors |
| `data_load` | CSV loading |
| `data_validate` | OHLC and date-order validation |
| `engine_execute` | Backtest core execution |
| `result_persist` | Persisting succeeded / failed task state |
| `rabbitmq_ack` | ACK operation for claimed deliveries |

The current implementation does not yet report end-to-end queue wait, full HTTP request latency, live PostgreSQL pool utilization, or RabbitMQ queue depth. Do not infer these values from adjacent stage timings.

## Sampling and overhead

Each stage keeps at most the latest 20,000 duration samples. The snapshot sorts a copy of each stage's samples to calculate percentiles; this work happens when `GET /api/metrics` is requested, not on every metric update. Counters use relaxed atomics, and duration samples share a mutex-protected map.

## Interpretation

- Counters are process-local and do not include tasks completed before this backend process started.
- `publish_total` counts attempts, not unique task IDs.
- `worker_active` is the number of tasks currently in the worker execution/persistence path, not the number of connected consumers.
- `rabbitmq_connected` is true while at least one publisher/consumer connection tracked by this process is open.
- Percentiles are calculated over the retained recent sample window, not over all historical requests.
