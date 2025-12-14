use std::sync::{
    atomic::{AtomicU8, Ordering},
    Mutex,
    OnceLock,
};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

static GLOBAL_LOG_LEVEL: OnceLock<AtomicU8> = OnceLock::new();
static GLOBAL_LOG_HANDLER: OnceLock<Mutex<Box<dyn LogHandler>>> = OnceLock::new();

impl LogLevel {
    fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARN",
            LogLevel::Error => "ERROR",
        }
    }

    fn from_u8(val: u8) -> LogLevel {
        match val {
            0 => LogLevel::Debug,
            1 => LogLevel::Info,
            2 => LogLevel::Warning,
            _ => LogLevel::Error,
        }
    }
}

fn current_global_level() -> LogLevel {
    let stored = GLOBAL_LOG_LEVEL
        .get_or_init(|| AtomicU8::new(LogLevel::Info as u8))
        .load(Ordering::Relaxed);
    LogLevel::from_u8(stored)
}

fn should_log(level: LogLevel) -> bool {
    level >= current_global_level()
}

pub fn set_global_log_level(level: LogLevel) {
    GLOBAL_LOG_LEVEL
        .get_or_init(|| AtomicU8::new(level as u8))
        .store(level as u8, Ordering::Relaxed);
}

pub trait LogHandler: Send + Sync {
    fn handle(&self, level: LogLevel, msg: &str);
}

pub struct ConsoleLogger;

impl LogHandler for ConsoleLogger {
    fn handle(&self, level: LogLevel, msg: &str) {
        if level == LogLevel::Error {
            eprintln!("{}", msg);
            return;
        }
        println!("{}", msg);
    }
}

fn global_handler<'a>() -> std::sync::MutexGuard<'a, Box<dyn LogHandler>> {
    GLOBAL_LOG_HANDLER
        .get_or_init(|| Mutex::new(Box::new(ConsoleLogger)))
        .lock()
        .expect("global logger poisoned")
}

pub fn set_global_log_handler(handler: Box<dyn LogHandler>) {
    let mut guard = GLOBAL_LOG_HANDLER
        .get_or_init(|| Mutex::new(Box::new(ConsoleLogger)))
        .lock()
        .expect("global logger poisoned");
    *guard = handler;
}

pub fn log(level: LogLevel, name: &str, msg: &str) {
    if !should_log(level) {
        return;
    }

    let cur_thread = std::thread::current().id();
    let formatted_msg = format!("[{}] [{:?}] [{}] {}", level.as_str(), cur_thread, name, msg);
    global_handler().handle(level, &formatted_msg);
}

pub fn debug(name: &str, msg: &str) {
    log(LogLevel::Debug, name, msg);
}

pub fn info(name: &str, msg: &str) {
    log(LogLevel::Info, name, msg);
}

pub fn warn(name: &str, msg: &str) {
    log(LogLevel::Warning, name, msg);
}

pub fn error(name: &str, msg: &str) {
    log(LogLevel::Error, name, msg);
}
