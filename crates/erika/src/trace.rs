use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) fn enabled() -> bool {
    env_flag("ERIKA_CLOCK_TRACE")
        || env_flag("ERIKA_DANMAKU_TRACE")
        || env_flag("ERIKA_PLAYBACK_TRACE")
}

pub(crate) fn log(line: impl AsRef<str>) {
    if !enabled() {
        return;
    }
    append_line(line.as_ref(), default_trace_path());
}

pub(crate) fn append_line(line: impl AsRef<str>, path: impl AsRef<Path>) {
    let line = line.as_ref();
    let path = path.as_ref();
    eprintln!("{line}");
    if let Err(error) = append_line_to_path(line, path) {
        let fallback = fallback_trace_path(path);
        if fallback.as_path() != path {
            let _ = append_line_to_path(line, &fallback);
        }
        let _ = error;
    }
}

fn append_line_to_path(line: &str, path: &Path) -> std::io::Result<()> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "{line}"))
}

fn fallback_trace_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_owned())
        .unwrap_or_else(|| "erika_playback_trace.log".into());
    env::temp_dir().join(file_name)
}

pub(crate) fn default_trace_path() -> PathBuf {
    if env_flag("ERIKA_PLAYBACK_TRACE") {
        env::var_os("ERIKA_PLAYBACK_TRACE_FILE")
            .or_else(|| env::var_os("ERIKA_DANMAKU_TRACE_FILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/erika_playback_trace.log"))
    } else {
        env::var_os("ERIKA_DANMAKU_TRACE_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/erika_danmaku_trace.log"))
    }
}

pub(crate) fn duration_label(value: Option<Duration>) -> String {
    value
        .map(|duration| format!("{:.3}", duration.as_secs_f64()))
        .unwrap_or_else(|| "-".to_string())
}

pub(crate) fn duration_regressed(next: Duration, previous: Duration) -> bool {
    previous
        .checked_sub(next)
        .is_some_and(|delta| delta > Duration::from_millis(5))
}

pub(crate) fn duration_diff(a: Duration, b: Duration) -> Duration {
    a.checked_sub(b)
        .or_else(|| b.checked_sub(a))
        .unwrap_or(Duration::ZERO)
}

pub(crate) fn env_flag(name: &str) -> bool {
    match env::var(name).ok().as_deref() {
        Some("0" | "false" | "FALSE" | "off" | "OFF" | "") | None => false,
        Some(_) => true,
    }
}
