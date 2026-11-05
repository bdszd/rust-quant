use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataQualityReport {
    pub total_rows: usize,

    pub accepted_rows: usize,

    pub rejected_rows: usize,

    pub duplicate_rows: usize,

    #[serde(default)]
    pub warning_rows: usize,

    #[serde(default)]
    pub rejection_reasons: BTreeMap<String, usize>,

    #[serde(default)]
    pub warning_reasons: BTreeMap<String, usize>,
}

impl DataQualityReport {
    pub fn record_rejection(&mut self, reasons: &[String]) {
        self.rejected_rows += 1;

        for reason in reasons {
            if reason == "duplicate trade date" {
                self.duplicate_rows += 1;
            }

            *self.rejection_reasons.entry(reason.clone()).or_insert(0) += 1;
        }
    }

    /// 记录一条行情上的异常提示。
    ///
    /// 同一条行情即使有多个警告，warning_rows 也只增加一次。
    pub fn record_warnings(&mut self, warnings: &[String]) {
        if warnings.is_empty() {
            return;
        }

        self.warning_rows += 1;

        for warning in warnings {
            *self.warning_reasons.entry(warning.clone()).or_insert(0) += 1;
        }
    }

    pub fn print_summary(&self) {
        println!("========== Data Quality Report ==========");

        println!("Total rows: {}", self.total_rows);
        println!("Accepted rows: {}", self.accepted_rows);
        println!("Rejected rows: {}", self.rejected_rows);
        println!("Duplicate rows: {}", self.duplicate_rows);
        println!("Warning rows: {}", self.warning_rows);

        println!("\nRejection reasons:");

        for (reason, count) in &self.rejection_reasons {
            println!("  {}: {}", reason, count);
        }

        println!("\nWarning reasons:");

        for (reason, count) in &self.warning_reasons {
            println!("  {}: {}", reason, count);
        }

        println!("=========================================");
    }
}
