# Backend monitoring

This stack adds Prometheus scraping and a provisioned Grafana dashboard to the existing Rust Quant backend.

## Start

Start the backend as usual, with PostgreSQL and RabbitMQ available:

```bash
docker compose up -d postgres rabbitmq
```

Run the backend from the repository root using the existing environment variables (`DATABASE_URL`, `AMQP_ADDR`, and optionally `RUST_QUANT_DATA_DIR`).

Then start the monitoring stack:

```bash
docker compose up -d prometheus grafana
```

## Endpoints

- Backend health: http://localhost:8000/api/health
- Existing JSON metrics: http://localhost:8000/api/metrics
- Prometheus scrape endpoint: http://localhost:8000/metrics
- Prometheus UI: http://localhost:9090
- Grafana: http://localhost:3000 (default local admin login: admin / admin; change it after first login)

Prometheus scrapes `host.docker.internal:8000/metrics` every five seconds. The backend binds to `0.0.0.0:8000` so the Prometheus container can reach it through the Docker host gateway.

## Current metric scope

The endpoint exports the counters and gauges already collected by the backend: task lifecycle counters, RabbitMQ publish/consume/ack counters, worker activity, RabbitMQ connection state, PostgreSQL pool connections, process uptime, and rolling-window stage duration statistics.

The stage duration values are exported as rolling-window gauges (avg/min/max/p50/p95/p99), not Prometheus histogram buckets. HTTP request rate and HTTP latency histograms are not included in this first iteration.

## Notes

- Existing PostgreSQL persistence, migrations, Outbox publishing, RabbitMQ delivery, and worker recovery are unchanged.
- Prometheus and Grafana data are stored in named Docker volumes.
- This setup is intended for local development; do not expose the default Grafana credentials or unauthenticated metrics endpoint to an untrusted network.
