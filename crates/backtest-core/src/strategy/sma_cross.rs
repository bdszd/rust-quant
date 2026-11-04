use super::{Signal, Strategy};
use crate::data::Bar;
use crate::indicators::sma;

pub struct SmaCrossStrategy {
    short_window: usize,
    long_window: usize,
    closes: Vec<f64>,
}

impl SmaCrossStrategy {
    pub fn new(short_window: usize, long_window: usize) -> Self {
        assert!(short_window > 0);
        assert!(long_window > short_window);

        Self {
            short_window,
            long_window,
            closes: Vec::new(),
        }
    }
}

impl Strategy for SmaCrossStrategy {
    fn on_bar(&mut self, bar: &Bar) -> Signal {
        self.closes.push(bar.close);

        if self.closes.len() < self.long_window + 1 {
            return Signal::Hold;
        }

        let len = self.closes.len();

        let short_now = sma(&self.closes, self.short_window);
        let long_now = sma(&self.closes, self.long_window);

        let short_prev = sma(&self.closes[..len - 1], self.short_window);
        let long_prev = sma(&self.closes[..len - 1], self.long_window);

        let (Some(short_now), Some(long_now), Some(short_prev), Some(long_prev)) =
            (short_now, long_now, short_prev, long_prev)
        else {
            return Signal::Hold;
        };

        if short_prev <= long_prev && short_now > long_now {
            Signal::Buy
        } else if short_prev >= long_prev && short_now < long_now {
            Signal::Sell
        } else {
            Signal::Hold
        }
    }
}
