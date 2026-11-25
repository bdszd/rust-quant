/// 指标计算结果。
///
/// 每个位置对应输入行情中的同一个位置。
/// None 表示当前数据点尚不能计算出有效指标值。
#[derive(Debug, Clone, PartialEq)]
pub struct IndicatorSeries {
    values: Vec<Option<f64>>,
}

impl IndicatorSeries {
    pub fn new(values: Vec<Option<f64>>) -> Self {
        Self { values }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// 获取指定位置的指标值。
    ///
    /// None 可能表示位置越界，也可能表示处于预热期。
    pub fn get(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied().flatten()
    }

    /// 获取完整指标序列。
    pub fn values(&self) -> &[Option<f64>] {
        &self.values
    }

    /// 获取第一个有效指标值的位置。
    pub fn first_valid_index(&self) -> Option<usize> {
        self.values.iter().position(Option::is_some)
    }
}
