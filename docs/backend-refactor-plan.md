# Backend Refactor Plan

This document records the agreed refactor boundary for the current backend.

Baseline: `84127a5219b581ce67ccc1fb3b647967be202762` on `feat/market-data`.
Work branch: `refactor/backend-architecture`.

## Scope

- Refactor `apps/backend` responsibilities and dependencies.
- Preserve the existing backtest request/response contract unless a change is required for correctness.
- Keep `crates/backtest-core` independent and unchanged.
- Keep the existing filesystem-backed cleaned dataset source for this refactor. The `market-data` crate is not silently wired into the backtest path.
- Do not extend Identity/Auth in this work.
- Add migration files as part of the code refactor, but do not execute migrations or alter any database volume.

## Target responsibilities

- **Backtest domain**: task, immutable job configuration, execution attempt, final result, and legal task-state transitions. No Axum, SQLx, RabbitMQ, filesystem root path, or Metrics dependency.
- **Backtest application**: use cases for creating/querying tasks and coordinating execution. Both HTTP handlers and workers call these use cases rather than implementing separate business flows.
- **Infrastructure**: PostgreSQL repository and RabbitMQ transport implementations.
- **HTTP**: request/response DTOs, handlers, middleware, and HTTP error mapping only.
- **Workers**: Outbox publisher and backtest consumer. They translate transport messages into application calls.
- **Bootstrap**: process configuration, dependency construction, migration invocation, worker startup, router startup, and shutdown lifecycle.

## Persistence model

The initial relational model has five tables:

1. `backtest_tasks`: task identity, optional `user_id` (nullable until owner isolation is connected), current status, creation/update timestamps.
2. `backtest_jobs`: one immutable execution configuration per task; `task_id` is unique and references the task. Runtime-only paths such as `data_dir` are not persisted.
3. `backtest_task_attempts`: one row per execution attempt, with attempt number, running/succeeded/failed/interrupted status, start/end timestamps, duration, and error. A task can have many attempts but at most one running attempt.
4. `backtest_results`: final result JSONB, one row per task.
5. `backtest_outbox`: independently identified events with task reference, event type, JSONB payload, creation/publication timestamps, and publisher-claim metadata. A task may emit more than one event.

The result JSON is versioned at the application boundary so future result-shape changes can be handled deliberately.

## Transaction boundaries

- Create task + job + initial outbox event in one PostgreSQL transaction.
- Claim a queued task + create its attempt in one transaction.
- Do not hold a database transaction while loading files or running the backtest engine.
- Persist result + successful attempt + successful task state in one transaction.
- Persist failed attempt + failed/retry task state in one transaction.
- Acknowledge RabbitMQ only after the corresponding database state is committed.
- Publisher marks an outbox event published only after a positive Publisher Confirm. Duplicate delivery remains possible; consumers must be idempotent. Delivery is at-least-once, not exactly-once.

## Recovery and concurrency

- Do not reset every `running` task at process startup: that is unsafe when multiple backend instances exist.
- A worker claim creates an attempt with a lease/heartbeat timestamp. Expired attempts can be marked interrupted and the task can be re-queued according to the retry policy.
- Outbox rows are claimed atomically with a lease so multiple publishers do not routinely publish the same pending row. A crash after broker confirmation but before marking published can still cause a duplicate, which is expected under at-least-once delivery.
- Invalid messages should be routed to a dead-letter queue rather than silently disappearing.

## Dependency direction

```text
HTTP Handler ─┐
              ├──> Backtest Application ───> Backtest Domain
Worker ───────┘             │
                            ├──> Repository port ───> PostgreSQL implementation
                            ├──> Dataset reader port ─> filesystem implementation
                            └──> Metrics port/observer

Worker Publisher ───> Outbox repository + RabbitMQ publisher
Bootstrap ──────────> constructs and wires all implementations
```

The domain and `backtest-core` do not know about HTTP, PostgreSQL, RabbitMQ, authentication, or process configuration.

## Explicit non-goals

- No changes to the backtest algorithms, portfolio engine, indicators, or trading rules.
- No Auth endpoint expansion or user ownership enforcement in this pass. The nullable owner field keeps the schema ready for later integration.
- No migration execution, database reset, Docker volume deletion, or runtime testing during the refactor.
- No market-data storage migration or automatic switch from CSV to PostgreSQL in this pass.
