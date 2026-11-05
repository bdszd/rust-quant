use std::error::Error;
use std::path::Path;

use super::cleaner::{CleaningResult, clean_tushare_file};

pub fn load_clean_bars(path: impl AsRef<Path>) -> Result<CleaningResult, Box<dyn Error>> {
    clean_tushare_file(path)
}
