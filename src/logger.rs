use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Arc;
use parking_lot::Mutex;
use once_cell::sync::OnceCell;
use chrono::Local;

static LOGGER: OnceCell<Arc<Mutex<File>>> = OnceCell::new();

pub fn reset_log_file(path: &str) -> anyhow::Result<()> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    Ok(())
}

pub fn init_logger(path: &str) -> anyhow::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    LOGGER.set(Arc::new(Mutex::new(file)))
        .map_err(|_| anyhow::anyhow!("Logger already initialized"))?;
    Ok(())
}

pub fn log_debug(message: &str) {
    if let Some(logger) = LOGGER.get() {
        let mut file = logger.lock();
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(file, "[{}] {}", timestamp, message);
    }
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        $crate::logger::log_debug(&format!($($arg)*));
    };
}
