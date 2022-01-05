use std::lazy::SyncLazy;
use std::sync::Mutex;

// const ANSI_RED: &str = "\x1b[41m";
// const ANSI_RESET: &str = "\x1b[0m";
pub const FAKE_ANSI: char = '\x1b';

static LOG_BUF: SyncLazy<Mutex<String>> = SyncLazy::new(|| Mutex::new(String::new()));

pub fn log__(string: String) {
    let mut buf = LOG_BUF.lock().unwrap();
    buf.push_str(&string);
    buf.push('\n');
}

pub fn log_err__(string: String) {
    let mut buf = LOG_BUF.lock().unwrap();
    buf.push(FAKE_ANSI);
    buf.push_str(&string);
    buf.push(FAKE_ANSI);
    buf.push('\n');
}

pub fn drain_log() -> String {
    std::mem::take(&mut LOG_BUF.lock().unwrap())
}

macro_rules! log {
    ($( $arg: expr ),*) => {
        $crate::logging::log__(format!("{}", format_args!($( $arg ),*) ))
    };
}
macro_rules! log_err {
    ($( $arg: expr ),*) => {
        $crate::logging::log_err__(format!("{}", format_args!($( $arg ),*) ))
    };
}
pub(crate) use log;
pub(crate) use log_err;
