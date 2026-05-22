pub mod cli;
pub mod format;
pub mod input;
pub mod query;
pub mod analysis;
pub mod output;

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

/// A single parsed log record, normalized across all formats.
#[derive(Debug, Clone)]
pub struct Record {
    /// Original line number in the source file
    pub line_number: usize,
    /// Timestamp (if detected)
    pub timestamp: Option<DateTime<Utc>>,
    /// Log level (if detected)
    pub level: Option<Level>,
    /// The main message text
    pub message: String,
    /// All structured fields (key-value pairs)
    pub fields: HashMap<String, Value>,
    /// The raw original line
    pub raw: String,
}

/// Normalized log levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Level {
    pub fn from_str_loose(s: &str) -> Option<Level> {
        match s.to_ascii_lowercase().as_str() {
            "trace" | "trc" | "verbose" => Some(Level::Trace),
            "debug" | "dbg" | "dbug" => Some(Level::Debug),
            "info" | "inf" | "information" => Some(Level::Info),
            "warn" | "warning" | "wrn" => Some(Level::Warn),
            "error" | "err" | "eror" | "fail" | "failure" => Some(Level::Error),
            "fatal" | "critical" | "crit" | "ftl" | "panic" | "emerg" | "emergency" => {
                Some(Level::Fatal)
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
            Level::Fatal => "FATAL",
        }
    }

    pub fn is_error_or_above(&self) -> bool {
        matches!(self, Level::Error | Level::Fatal)
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Detected log format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// JSON Lines (one JSON object per line)
    JsonLines,
    /// logfmt (key=value pairs)
    Logfmt,
    /// Common Log Format (Apache/nginx)
    CommonLog,
    /// Combined Log Format (CLF + referer + user-agent)
    CombinedLog,
    /// Syslog (RFC 3164 / RFC 5424)
    Syslog,
    /// Unstructured text (fallback)
    Plain,
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Format::JsonLines => write!(f, "JSON Lines"),
            Format::Logfmt => write!(f, "logfmt"),
            Format::CommonLog => write!(f, "Common Log Format"),
            Format::CombinedLog => write!(f, "Combined Log Format"),
            Format::Syslog => write!(f, "syslog"),
            Format::Plain => write!(f, "plain text"),
        }
    }
}
