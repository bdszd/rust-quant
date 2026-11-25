use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Task 是用户提交的一次回测请求。
///
/// 它只保存任务身份和当前状态，不包含执行配置、数据库行或 HTTP 字段。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BacktestTask {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

impl BacktestTask {
    pub fn can_transition_to(&self, next: TaskStatus) -> bool {
        matches!(
            (self.status, next),
            (TaskStatus::Queued, TaskStatus::Running)
                | (TaskStatus::Running, TaskStatus::Queued)
                | (TaskStatus::Running, TaskStatus::Succeeded)
                | (TaskStatus::Running, TaskStatus::Failed)
                | (TaskStatus::Failed, TaskStatus::Queued)
        )
    }

    pub fn transition(&mut self, next: TaskStatus) -> Result<(), String> {
        if !self.can_transition_to(next) {
            return Err(format!(
                "不允许任务状态从 {} 转换为 {}",
                self.status.as_str(),
                next.as_str()
            ));
        }

        self.status = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_expected_task_transitions() {
        let mut task = BacktestTask {
            id: Uuid::nil(),
            user_id: None,
            status: TaskStatus::Queued,
        };

        assert!(task.transition(TaskStatus::Running).is_ok());
        assert!(task.transition(TaskStatus::Succeeded).is_ok());
        assert!(task.transition(TaskStatus::Queued).is_err());
    }

    #[test]
    fn allows_retry_after_failure() {
        let mut task = BacktestTask {
            id: Uuid::nil(),
            user_id: None,
            status: TaskStatus::Failed,
        };

        assert!(task.transition(TaskStatus::Queued).is_ok());
    }
}
