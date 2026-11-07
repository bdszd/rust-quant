
import http from 'k6/http';
import { check, sleep } from 'k6';
import { Counter, Trend } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:8000';
const DATASET = __ENV.DATASET;

// 任务统计
const acceptedTasks = new Counter('accepted_tasks');
const completedTasks = new Counter('completed_tasks');
const failedTasks = new Counter('failed_tasks');
const timeoutTasks = new Counter('timeout_tasks');
const pollErrors = new Counter('poll_errors');

// 耗时统计
const createDuration = new Trend('create_task_duration', true);
const completionDuration = new Trend('task_completion_duration', true);

// 查询次数
const pollRequests = new Counter('task_poll_requests');

// 最长等待任务完成时间
const TASK_TIMEOUT = 180000;

// 每次查询之间等待 1 秒
const POLL_INTERVAL = 1;

if (!DATASET) {
    throw new Error('请通过 -e DATASET=实际文件名.csv 指定数据集');
}

export const options = {
    stages: [
        { duration: '10s', target: 3 },
        { duration: '20s', target: 5 },
        { duration: '10s', target: 10 },
        { duration: '10s', target: 0 },
    ],

    thresholds: {
        http_req_failed: ['rate<0.01'],
        create_task_duration: ['p(95)<1000'],
        poll_errors: ['count==0'],
        timeout_tasks: ['count==0'],
    },
};

export default function () {
    // 记录整个任务生命周期的开始时间
    const startTime = Date.now();

    // 第一阶段：创建任务
    const payload = {
        dataset: DATASET,
        initial_cash: 100000,
        commission_rate: 0.001,
        slippage_rate: 0.001,
        execution_mode: 'next_open',
        strategy: 'sma_cross',
        short_window: 2,
        long_window: 3,
    };

    const createResponse = http.post(
        `${BASE_URL}/api/backtests`,
        JSON.stringify(payload),
        {
            headers: {
                'Content-Type': 'application/json',
            },
        }
    );

    createDuration.add(createResponse.timings.duration);

    let taskId;

    try {
        taskId = createResponse.json('id');
    } catch (_) {
        taskId = null;
    }

    const created = check(createResponse, {
        'create status is 202': (r) => r.status === 202,
        'task id exists': () => Boolean(taskId),
    });

    if (!created) {
        console.error(
            `创建任务失败: status=${createResponse.status}, body=${createResponse.body}`
        );
        return;
    }

    acceptedTasks.add(1);

    // 第二阶段：循环查询任务状态
    const deadline = Date.now() + TASK_TIMEOUT;

    while (Date.now() < deadline) {
        sleep(POLL_INTERVAL);

        const response = http.get(
            `${BASE_URL}/api/backtests/${taskId}`
        );

        pollRequests.add(1);

        if (response.status !== 200) {
            pollErrors.add(1);
            console.error(
                `查询任务失败: id=${taskId}, status=${response.status}`
            );
            return;
        }

        let status;

        try {
            status = response.json('status');
        } catch (_) {
            pollErrors.add(1);
            console.error(`无法解析任务状态: id=${taskId}`);
            return;
        }

        // 任务成功
        if (status === 'succeeded') {
            completedTasks.add(1);
            completionDuration.add(Date.now() - startTime);

            console.log(`任务完成: ${taskId}`);
            return;
        }

        // 任务失败
        if (status === 'failed') {
            failedTasks.add(1);
            completionDuration.add(Date.now() - startTime);

            console.error(
                `任务执行失败: id=${taskId}, error=${response.json('error')}`
            );
            return;
        }

        // 任务仍在排队或执行中
        if (status !== 'queued' && status !== 'running') {
            pollErrors.add(1);
            console.error(`未知任务状态: id=${taskId}, status=${status}`);
            return;
        }
    }

    // 超过等待时间仍未完成
    timeoutTasks.add(1);
    console.error(`任务等待超时: ${taskId}`);
}
