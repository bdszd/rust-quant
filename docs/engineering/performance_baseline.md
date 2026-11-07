# Rust Quant 性能测试记录

## 1. 测试目的

验证当前 Rust Quant 回测系统在并发任务提交场景下的稳定性和处理能力。

重点测试：

- API 并发请求处理能力
- Outbox 任务发布能力
- RabbitMQ 消息传递
- Worker 并发消费与执行
- PostgreSQL 任务状态及结果持久化
- 端到端任务完成情况

本次测试主要建立系统的第一版性能基线，不以测出系统极限吞吐量为目标。

## 2. 当前系统组件与并发配置

当前系统架构：

```text
                  Client / k6
                       |
                       v
                 Axum API Gateway
                       |
                       v
                  PostgreSQL
                  /         \
         backtest_tasks   backtest_outbox
                              |
                              v
                       Outbox Publisher
                              |
                              v
                          RabbitMQ
                              |
                    backtest.jobs Queue
                         /        \
                        v          v
                    Worker 0   Worker 1
                        \          /
                         v        v
                        Backtest Core
                              |
                              v
                         PostgreSQL
                         结果持久化
```

### 组件配置

| 组件 | 当前配置 | 说明 |
|---|---:|---|
| Axum API | 1 个 Backend 实例 | 接收 HTTP 请求 |
| API 压测并发 | 最高 50 VU | k6 模拟 50 个虚拟用户 |
| Outbox Publisher | 1 个 | 定期扫描待发布任务 |
| Publisher 轮询间隔 | 500ms | 每次最多处理 32 条 |
| RabbitMQ Queue | 1 个 | `backtest.jobs` |
| Worker | 2 个 | 同时消费和执行任务 |
| RabbitMQ Consumer | 2 个 | 每个 Worker 一个 Consumer |
| Consumer Prefetch | 1 | 每个 Consumer 最多持有一条未 ACK 消息 |
| PostgreSQL | 1 个实例 | 保存任务、Outbox 和回测结果 |

说明：

1. 当前 Worker 数量为 2，因此最多有 2 个 Worker 同时执行回测任务。
2. Publisher 和 Worker 是独立的后台任务。
3. PostgreSQL 的连接池大小尚未纳入本次性能记录。
4. Axum Runtime 的线程数也尚未单独测量。
5. 50 VU 是压测客户端的虚拟用户数量，不代表 API 内部只有 50 个线程。

## 3. 测试环境

- 操作系统：Ubuntu
- Rust：1.96.0
- Web Framework：Axum
- 数据库：PostgreSQL 17
- 消息队列：RabbitMQ 4
- 压测工具：k6
- 回测数据：000001.SZ
- 数据区间：2024-01-01 至 2024-12-31
- 数据量：242 行日线数据

## 4. API 提交压力测试

测试脚本：

```text
tests/load/backtest.js
```

该测试只关注任务提交，不等待任务执行完成。

### 测试结果

| 指标 | 5 VU | 20 VU | 50 VU |
|---|---:|---:|---:|
| 成功提交任务 | 105 | 419 | 1030 |
| 提交吞吐量 | 2.08/s | 8.31/s | 20.37/s |
| 平均响应时间 | 9.31ms | 7.32ms | 7.31ms |
| P95 响应时间 | 16.17ms | 14.12ms | 13.76ms |
| HTTP 错误率 | 0% | 0% | 0% |
| 拒绝任务数 | 0 | 0 | 0 |

### 结果分析

在最高 50 VU 的压力下：

- 成功提交 1030 个任务。
- 提交吞吐量达到约 20.37 tasks/s。
- API P95 响应时间为 13.76ms。
- 没有 HTTP 请求失败。
- 没有任务被拒绝。

当前 API 接收能力表现稳定，没有观察到明显的响应时间恶化。

注意：该测试不能单独证明 Worker 具有 20 tasks/s 的处理能力。

## 5. 端到端压力测试

测试脚本：

```text
tests/load/backtest_e2e.js
```

端到端测试会：

1. 提交回测任务。
2. 获取任务 ID。
3. 定期轮询任务状态。
4. 等待任务进入最终状态。
5. 统计任务完成情况和耗时。

### 测试结果

| 指标 | 3 VU | 20 VU | 50 VU |
|---|---:|---:|---:|
| 成功提交 | 71 | 418 | 1008 |
| 成功完成 | 71 | 418 | 1008 |
| 完成率 | 100% | 100% | 100% |
| 提交吞吐量 | 1.39/s | 8.24/s | 20.04/s |
| 创建任务 P95 | 12.39ms | 13.91ms | 21.42ms |
| HTTP 错误率 | 0% | 0% | 0% |
| 轮询错误 | 0 | 0 | 0 |
| 超时任务 | 0 | 0 | 0 |

### 50 VU 测试的详细数据

```text
accepted_tasks:          1008
completed_tasks:         1008

create_task_duration:
  avg:                   8.31ms
  p90:                   15.43ms
  p95:                   21.42ms
  max:                   97.82ms

task_completion_duration:
  avg:                   1.03s
  p95:                   1.04s
  max:                   2.03s

http_req_failed:         0%
poll_errors:             0
timeout_tasks:           0
```

注意：

当前脚本每隔 1 秒轮询一次任务状态。

因此，约 1 秒的任务完成时间包含轮询等待时间，不能直接当作 Worker 的真实执行耗时。

## 6. 消息队列检查

50 VU 端到端测试结束后，RabbitMQ 状态：

```text
name           messages_ready
               messages_unacknowledged
               consumers

backtest.jobs  0
               0
               2
```

结果：

| 指标 | 结果 |
|---|---:|
| Ready Messages | 0 |
| Unacknowledged Messages | 0 |
| Consumers | 2 |

说明测试结束后没有消息积压，两个 Consumer 正常运行。

## 7. 数据库任务状态检查

测试结束后执行：

```sql
SELECT status, COUNT(*)
FROM backtest_tasks
GROUP BY status
ORDER BY status;
```

最终结果：

```text
status      count
-----------------
succeeded   3159
```

数据库中：

- succeeded：3159
- failed：0
- queued：0
- running：0

注意：3159 是数据库累计任务数量，包含此前测试产生的任务，不是单独这一轮测试的任务数。

## 8. 当前性能基线

当前系统在以下条件下通过了测试：

| 项目 | 实测结果 |
|---|---:|
| API 最大测试并发 | 50 VU |
| API 提交吞吐量 | 20.37 tasks/s |
| 端到端提交吞吐量 | 20.04 tasks/s |
| 50 VU API 创建任务 P95 | 13.76ms |
| 50 VU 端到端创建任务 P95 | 21.42ms |
| 端到端任务完成率 | 100% |
| HTTP 错误率 | 0% |
| 任务超时数量 | 0 |
| 任务失败数量 | 0 |
| 测试结束后 RabbitMQ 积压 | 0 |

## 9. 当前结论与限制

### 已验证

1. API 能够稳定接收并发任务提交。
2. Transactional Outbox 能够将任务可靠地交给 Publisher。
3. Publisher 能够向 RabbitMQ 发布任务。
4. 两个 Worker 能够正常消费并执行任务。
5. PostgreSQL 能够保存任务状态和回测结果。
6. 当前测试负载下没有观察到任务丢失、失败或超时。
7. 测试结束后，RabbitMQ 队列和数据库均没有遗留任务。

### 尚未验证

1. 系统的最大任务吞吐量。
2. Worker 数量增加后的扩展能力。
3. PostgreSQL 连接池的性能瓶颈。
4. 更大规模数据集下的回测执行能力。
5. 长时间持续运行时的稳定性。
6. 多 Backend 实例部署时的任务恢复和协调能力。
7. 真实 Worker 进程异常退出后的 RabbitMQ 消息重新投递行为。

### 后续性能优化方向

- 测量 Worker 实际执行吞吐量。
- 测试不同 Worker 数量下的吞吐量变化。
- 增加数据规模，观察 CPU、内存和数据库负载。
- 测量任务排队时间、执行时间、持久化时间。
- 进一步测试 100 VU 及更高并发。

---

**测试阶段：第一版性能基线**

**测试结论：当前系统在最高 50 VU、约 20 tasks/s 的测试负载下，端到端任务处理正常，所有观测到的任务均成功完成。**