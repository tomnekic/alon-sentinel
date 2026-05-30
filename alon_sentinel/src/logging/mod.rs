use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

const LOG_FILENAME_PREFIX: &str = "site_monitor.log";

pub fn init_logging(log_dir: &Path, log_level: &str, max_files: usize) -> WorkerGuard {
    std::fs::create_dir_all(log_dir).expect("failed to create log directory");
    prune_old_log_files(log_dir, max_files);

    let file_appender = tracing_appender::rolling::daily(log_dir, LOG_FILENAME_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .init();

    guard
}

pub fn prune_old_log_files(log_dir: &Path, max_files: usize) {
    if max_files == 0 {
        return;
    }

    let mut log_files: Vec<_> = match std::fs::read_dir(log_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(LOG_FILENAME_PREFIX)
            })
            .map(|e| e.path())
            .collect(),
        Err(_) => return,
    };

    log_files.sort();

    let excess = log_files.len().saturating_sub(max_files);
    for path in log_files.iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}
