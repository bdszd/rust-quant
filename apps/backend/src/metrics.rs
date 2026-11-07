use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_SAMPLES_PER_STAGE: usize = 20_000;

/// 进程级性能统计。每个阶段只保留最近 20,000 个样本，避免压测期间无限增长。
pub struct Metrics {
    started_at: Instant,
    worker_count: AtomicUsize,
    active_workers: AtomicUsize,
    tasks_accepted: AtomicU64,
    tasks_succeeded: AtomicU64,
    tasks_failed: AtomicU64,
    tasks_duplicate: AtomicU64,
    publish_total: AtomicU64,
    publish_failed: AtomicU64,
    publish_confirm_failed: AtomicU64,
    consume_total: AtomicU64,
    ack_total: AtomicU64,
    ack_failed: AtomicU64,
    reject_total: AtomicU64,
    rabbitmq_connected: AtomicUsize,
    stages: Mutex<BTreeMap<String, VecDeque<f64>>>,
}

#[derive(Debug, Serialize)]
pub struct StageStats {
    pub count: usize,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

/// 某一时刻 PostgreSQL 连接池的状态。
#[derive(Debug, Serialize)]
pub struct DatabasePoolStats {
    /// 当前已经建立的连接数。
    pub size: usize,
    /// 当前空闲连接数。
    pub idle: usize,
    /// 当前正在使用的连接数（size - idle）。
    pub busy: usize,
    /// 连接池允许建立的最大连接数。
    pub max_connections: usize,
}

#[derive(Debug, Serialize)]
pub struct MetricsSnapshot {
    pub uptime_seconds: f64,
    pub worker_count: usize,
    pub worker_active: usize,
    pub rabbitmq_connected: bool,
    pub database_pool: DatabasePoolStats,
    pub tasks: TaskCounters,
    pub messaging: MessagingCounters,
    pub stages: BTreeMap<String, StageStats>,
}

#[derive(Debug, Serialize)]
pub struct TaskCounters {
    pub accepted: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub duplicate: u64,
}

#[derive(Debug, Serialize)]
pub struct MessagingCounters {
    pub publish_total: u64,
    pub publish_failed: u64,
    pub publish_confirm_failed: u64,
    pub consume_total: u64,
    pub ack_total: u64,
    pub ack_failed: u64,
    pub reject_total: u64,
}

impl Metrics {
    pub fn new(worker_count: usize) -> Self {
        Self {
            started_at: Instant::now(),
            worker_count: AtomicUsize::new(worker_count),
            active_workers: AtomicUsize::new(0),
            tasks_accepted: AtomicU64::new(0),
            tasks_succeeded: AtomicU64::new(0),
            tasks_failed: AtomicU64::new(0),
            tasks_duplicate: AtomicU64::new(0),
            publish_total: AtomicU64::new(0),
            publish_failed: AtomicU64::new(0),
            publish_confirm_failed: AtomicU64::new(0),
            consume_total: AtomicU64::new(0),
            ack_total: AtomicU64::new(0),
            ack_failed: AtomicU64::new(0),
            reject_total: AtomicU64::new(0),
            rabbitmq_connected: AtomicUsize::new(0),
            stages: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn record_duration(&self, stage: &'static str, elapsed: Duration) {
        let value = elapsed.as_secs_f64() * 1000.0;

        if let Ok(mut stages) = self.stages.lock() {
            let samples = stages.entry(stage.to_string()).or_default();

            if samples.len() == MAX_SAMPLES_PER_STAGE {
                samples.pop_front();
            }

            samples.push_back(value);
        }
    }

    pub fn increment_tasks_accepted(&self) {
        self.tasks_accepted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_tasks_succeeded(&self) {
        self.tasks_succeeded.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_tasks_failed(&self) {
        self.tasks_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_tasks_duplicate(&self) {
        self.tasks_duplicate.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_publish_total(&self) {
        self.publish_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_publish_failed(&self) {
        self.publish_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_publish_confirm_failed(&self) {
        self.publish_confirm_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_consume_total(&self) {
        self.consume_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_ack_total(&self) {
        self.ack_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_ack_failed(&self) {
        self.ack_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_reject_total(&self) {
        self.reject_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn worker_started(&self) {
        self.active_workers.fetch_add(1, Ordering::Relaxed);
    }

    pub fn worker_finished(&self) {
        self.active_workers.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn rabbitmq_connection_opened(&self) {
        self.rabbitmq_connected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn rabbitmq_connection_closed(&self) {
        let _ =
            self.rabbitmq_connected
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                    Some(count.saturating_sub(1))
                });
    }

    pub fn snapshot(&self, database_pool: DatabasePoolStats) -> MetricsSnapshot {
        let stages = self
            .stages
            .lock()
            .map(|guard| {
                guard
                    .iter()
                    .filter_map(|(name, samples)| {
                        (!samples.is_empty()).then(|| (name.clone(), calculate_stats(samples)))
                    })
                    .collect()
            })
            .unwrap_or_default();

        MetricsSnapshot {
            uptime_seconds: self.started_at.elapsed().as_secs_f64(),
            worker_count: self.worker_count.load(Ordering::Relaxed),
            worker_active: self.active_workers.load(Ordering::Relaxed),
            rabbitmq_connected: self.rabbitmq_connected.load(Ordering::Relaxed) > 0,
            database_pool,
            tasks: TaskCounters {
                accepted: self.tasks_accepted.load(Ordering::Relaxed),
                succeeded: self.tasks_succeeded.load(Ordering::Relaxed),
                failed: self.tasks_failed.load(Ordering::Relaxed),
                duplicate: self.tasks_duplicate.load(Ordering::Relaxed),
            },
            messaging: MessagingCounters {
                publish_total: self.publish_total.load(Ordering::Relaxed),
                publish_failed: self.publish_failed.load(Ordering::Relaxed),
                publish_confirm_failed: self.publish_confirm_failed.load(Ordering::Relaxed),
                consume_total: self.consume_total.load(Ordering::Relaxed),
                ack_total: self.ack_total.load(Ordering::Relaxed),
                ack_failed: self.ack_failed.load(Ordering::Relaxed),
                reject_total: self.reject_total.load(Ordering::Relaxed),
            },
            stages,
        }
    }
}

fn calculate_stats(samples: &VecDeque<f64>) -> StageStats {
    let mut sorted: Vec<f64> = samples.iter().copied().collect();
    sorted.sort_by(f64::total_cmp);

    let count = sorted.len();
    let sum: f64 = sorted.iter().sum();

    StageStats {
        count,
        avg_ms: sum / count as f64,
        min_ms: sorted[0],
        max_ms: sorted[count - 1],
        p50_ms: percentile(&sorted, 0.50),
        p95_ms: percentile(&sorted, 0.95),
        p99_ms: percentile(&sorted, 0.99),
    }
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * quantile).ceil() as usize;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_percentiles_and_keeps_bounded_samples() {
        let metrics = Metrics::new(2);

        for millis in 1..=MAX_SAMPLES_PER_STAGE + 1 {
            metrics.record_duration("test_stage", Duration::from_millis(millis as u64));
        }

        let snapshot = metrics.snapshot(DatabasePoolStats {
            size: 10,
            idle: 10,
            busy: 0,
            max_connections: 10,
        });

        let stage = &snapshot.stages["test_stage"];

        assert_eq!(stage.count, MAX_SAMPLES_PER_STAGE);
        assert_eq!(stage.min_ms, 2.0);
        assert_eq!(stage.max_ms, MAX_SAMPLES_PER_STAGE as f64 + 1.0);
    }
}
