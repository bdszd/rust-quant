
import http from 'k6/http';
import { check, sleep } from 'k6';
import { Counter, Trend } from 'k6/metrics';

// 统计成功创建和失败的任务数量
const acceptedTasks = new Counter('accepted_tasks');
const rejectedTasks = new Counter('rejected_tasks');

// 单独统计创建任务的 HTTP 响应耗时
const createTaskDuration = new Trend('create_task_duration');

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:8000';
const DATASET = __ENV.DATASET;

if (!DATASET) {
    throw new Error('请通过 -e DATASET=实际文件名.csv 指定数据集');
}

export const options = {
    stages: [
        { duration: '10s', target: 1 },
        { duration: '20s', target: 3 },
        { duration: '10s', target: 5 },
        { duration: '10s', target: 0 },
    ],

    thresholds: {
        http_req_failed: ['rate<0.01'],
        http_req_duration: ['p(95)<1000'],
        rejected_tasks: ['count==0'],
    },
};

export default function () {
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

    const response = http.post(
        `${BASE_URL}/api/backtests`,
        JSON.stringify(payload),
        {
            headers: {
                'Content-Type': 'application/json',
            },
        }
    );

    createTaskDuration.add(response.timings.duration);

    const success = check(response, {
        'status is 202': (r) => r.status === 202,
        'response has task id': (r) => {
            try {
                return Boolean(r.json('id'));
            } catch (_) {
                return false;
            }
        },
        'task status is queued': (r) => {
            try {
                return r.json('status') === 'queued';
            } catch (_) {
                return false;
            }
        },
    });

    if (success) {
        acceptedTasks.add(1);
    } else {
        rejectedTasks.add(1);
        console.error(
            `创建任务失败: status=${response.status}, body=${response.body}`
        );
    }

    sleep(1);
}
