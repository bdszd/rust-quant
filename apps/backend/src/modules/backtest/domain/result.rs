use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// 回测结果以带版本号的 JSON 保存。
///
/// 版本号用于未来调整结果结构时区分旧数据和新数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResultRecord {
    pub task_id: Uuid,
    pub schema_version: i32,
    pub payload: Value,
}
