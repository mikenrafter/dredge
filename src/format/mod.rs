use crate::{Format, Level, Record};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Compiled regexes
// ---------------------------------------------------------------------------

static CLF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(\S+) (\S+) (\S+) \[([^\]]+)\] "(\S+) (\S+)(?: (\S+))?" (\d{3}) (\S+)$"#,
    )
    .unwrap()
});

static COMBINED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(\S+) (\S+) (\S+) \[([^\]]+)\] "(\S+) (\S+)(?: (\S+))?" (\d{3}) (\S+) "([^"]*)" "([^"]*)"$"#,
    )
    .unwrap()
});

static SYSLOG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:<(\d+)>)?([A-Z][a-z]{2})\s+([\d ]\d)\s+(\d{2}:\d{2}:\d{2})\s+(\S+)\s+(\S+?)(?:\[(\d+)\])?:\s+(.*)",
    )
    .unwrap()
});

static LOGFMT_PAIR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(\w[\w.]*)=((?:"(?:[^"\\]|\\.)*")|(?:\S*))"#).unwrap()
});

static PLAIN_LEVEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL|CRITICAL|PANIC|EMERG(?:ENCY)?)\b")
        .unwrap()
});

static PLAIN_BRACKET_LEVEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\[(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL|CRITICAL|PANIC|EMERG(?:ENCY)?)\]")
        .unwrap()
});

// Timestamp at start of plain lines: ISO 8601 / RFC 3339 style
static PLAIN_TS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)")
        .unwrap()
});

// ---------------------------------------------------------------------------
// Timestamp field name heuristics
// ---------------------------------------------------------------------------

const TIMESTAMP_FIELDS: &[&str] = &[
    "timestamp",
    "time",
    "ts",
    "@timestamp",
    "datetime",
    "date",
    "t",
];

const LEVEL_FIELDS: &[&str] = &[
    "level",
    "severity",
    "log_level",
    "loglevel",
    "lvl",
    "log.level",
];

const MESSAGE_FIELDS: &[&str] = &["message", "msg", "log", "text", "body"];

// ---------------------------------------------------------------------------
// Format detection
// ---------------------------------------------------------------------------

/// Detect the log format by sampling the first non-empty lines.
pub fn detect(lines: &[&str]) -> Format {
    let samples: Vec<&str> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .take(10)
        .copied()
        .collect();

    if samples.is_empty() {
        return Format::Plain;
    }

    let mut json_count = 0;
    let mut logfmt_count = 0;
    let mut combined_count = 0;
    let mut clf_count = 0;
    let mut syslog_count = 0;

    for line in &samples {
        let trimmed = line.trim();

        // JSON Lines check
        if trimmed.starts_with('{') {
            if serde_json::from_str::<Value>(trimmed).is_ok() {
                json_count += 1;
                continue;
            }
        }

        // Combined before CLF (combined is a superset)
        if COMBINED_RE.is_match(trimmed) {
            combined_count += 1;
            continue;
        }

        if CLF_RE.is_match(trimmed) {
            clf_count += 1;
            continue;
        }

        // Syslog check
        if SYSLOG_RE.is_match(trimmed) || trimmed.starts_with('<') {
            syslog_count += 1;
            continue;
        }

        // logfmt check: at least 2 key=value pairs
        let pairs: usize = LOGFMT_PAIR_RE.find_iter(trimmed).count();
        if pairs >= 2 {
            logfmt_count += 1;
        }
    }

    let total = samples.len();
    let threshold = total / 2; // majority wins

    if json_count > threshold {
        Format::JsonLines
    } else if combined_count > threshold {
        Format::CombinedLog
    } else if clf_count > threshold {
        Format::CommonLog
    } else if syslog_count > threshold {
        Format::Syslog
    } else if logfmt_count > threshold {
        Format::Logfmt
    } else {
        Format::Plain
    }
}

// ---------------------------------------------------------------------------
// Timestamp parsing helpers
// ---------------------------------------------------------------------------

fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();

    // RFC 3339 / ISO 8601 with timezone
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // RFC 2822
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // ISO 8601 variants
    for fmt in &[
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M:%S%.fZ",
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S%:z",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
        // Try without timezone (assume UTC)
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(Utc.from_utc_datetime(&ndt));
        }
    }

    // CLF format: dd/Mon/yyyy:HH:MM:SS +zone
    if let Ok(dt) = DateTime::parse_from_str(s, "%d/%b/%Y:%H:%M:%S %z") {
        return Some(dt.with_timezone(&Utc));
    }

    // Syslog: Mon dd HH:MM:SS (no year — assume current year)
    for fmt in &["%b %d %H:%M:%S", "%b  %d %H:%M:%S"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(
            &format!("{} {}", Utc::now().format("%Y"), s),
            &format!("%Y {}", fmt),
        ) {
            return Some(Utc.from_utc_datetime(&ndt));
        }
    }

    // Unix epoch: seconds (10 digits) or milliseconds (13 digits)
    if let Ok(n) = s.parse::<i64>() {
        if (1_000_000_000..10_000_000_000).contains(&n) {
            // seconds
            if let Some(dt) = DateTime::from_timestamp(n, 0) {
                return Some(dt);
            }
        } else if (1_000_000_000_000..10_000_000_000_000).contains(&n) {
            // milliseconds
            let secs = n / 1000;
            let nsecs = ((n % 1000) * 1_000_000) as u32;
            if let Some(dt) = DateTime::from_timestamp(secs, nsecs) {
                return Some(dt);
            }
        }
    }

    // Also try float epoch
    if let Ok(f) = s.parse::<f64>() {
        if (1_000_000_000.0..10_000_000_000.0).contains(&f) {
            let secs = f as i64;
            let nsecs = ((f - secs as f64) * 1e9) as u32;
            if let Some(dt) = DateTime::from_timestamp(secs, nsecs) {
                return Some(dt);
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Per-format parsers
// ---------------------------------------------------------------------------

fn parse_json_line(line: &str, line_number: usize) -> Option<Record> {
    let obj: serde_json::Map<String, Value> = serde_json::from_str(line.trim()).ok()?;

    let mut timestamp = None;
    let mut level = None;
    let mut message = String::new();
    let mut fields = HashMap::new();

    let mut ts_key = None;
    let mut level_key = None;
    let mut msg_key = None;

    // Find well-known fields (case-insensitive)
    for key in obj.keys() {
        let lower = key.to_ascii_lowercase();
        if ts_key.is_none() && TIMESTAMP_FIELDS.contains(&lower.as_str()) {
            ts_key = Some(key.clone());
        }
        if level_key.is_none() && LEVEL_FIELDS.contains(&lower.as_str()) {
            level_key = Some(key.clone());
        }
        if msg_key.is_none() && MESSAGE_FIELDS.contains(&lower.as_str()) {
            msg_key = Some(key.clone());
        }
    }

    for (key, val) in &obj {
        if Some(key) == ts_key.as_ref() {
            let ts_str = match val {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => val.to_string(),
            };
            timestamp = parse_timestamp(&ts_str);
        } else if Some(key) == level_key.as_ref() {
            if let Value::String(s) = val {
                level = Level::from_str_loose(s);
            }
        } else if Some(key) == msg_key.as_ref() {
            if let Value::String(s) = val {
                message = s.clone();
            } else {
                message = val.to_string();
            }
        } else {
            fields.insert(key.clone(), val.clone());
        }
    }

    Some(Record {
        line_number,
        timestamp,
        level,
        message,
        fields,
        raw: line.to_string(),
    })
}

fn parse_logfmt_line(line: &str, line_number: usize) -> Option<Record> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for cap in LOGFMT_PAIR_RE.captures_iter(line) {
        let key = cap[1].to_string();
        let mut val = cap[2].to_string();
        // Strip surrounding quotes
        if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
            val = val[1..val.len() - 1].to_string();
            // Unescape basic sequences
            val = val.replace("\\\"", "\"").replace("\\\\", "\\");
        }
        pairs.push((key, val));
    }

    if pairs.is_empty() {
        return None;
    }

    let mut timestamp = None;
    let mut level = None;
    let mut message = String::new();
    let mut fields = HashMap::new();

    let mut ts_found = false;
    let mut level_found = false;
    let mut msg_found = false;

    for (key, val) in &pairs {
        let lower = key.to_ascii_lowercase();

        if !ts_found && TIMESTAMP_FIELDS.contains(&lower.as_str()) {
            timestamp = parse_timestamp(val);
            ts_found = true;
        } else if !level_found && LEVEL_FIELDS.contains(&lower.as_str()) {
            level = Level::from_str_loose(val);
            level_found = true;
        } else if !msg_found && MESSAGE_FIELDS.contains(&lower.as_str()) {
            message = val.clone();
            msg_found = true;
        } else {
            fields.insert(key.clone(), Value::String(val.clone()));
        }
    }

    Some(Record {
        line_number,
        timestamp,
        level,
        message,
        fields,
        raw: line.to_string(),
    })
}

fn parse_clf_line(line: &str, line_number: usize) -> Option<Record> {
    let caps = CLF_RE.captures(line.trim())?;

    let remote_host = caps.get(1).map(|m| m.as_str()).unwrap_or("-");
    let ident = caps.get(2).map(|m| m.as_str()).unwrap_or("-");
    let auth_user = caps.get(3).map(|m| m.as_str()).unwrap_or("-");
    let ts_str = caps.get(4).map(|m| m.as_str()).unwrap_or("");
    let method = caps.get(5).map(|m| m.as_str()).unwrap_or("");
    let path = caps.get(6).map(|m| m.as_str()).unwrap_or("");
    let protocol = caps.get(7).map(|m| m.as_str()).unwrap_or("");
    let status = caps.get(8).map(|m| m.as_str()).unwrap_or("");
    let size = caps.get(9).map(|m| m.as_str()).unwrap_or("");

    let timestamp = parse_timestamp(ts_str);

    let message = format!("{} {} {}", method, path, protocol).trim().to_string();

    let mut fields = HashMap::new();
    fields.insert("remote_host".into(), Value::String(remote_host.into()));
    if ident != "-" {
        fields.insert("ident".into(), Value::String(ident.into()));
    }
    if auth_user != "-" {
        fields.insert("auth_user".into(), Value::String(auth_user.into()));
    }
    fields.insert("method".into(), Value::String(method.into()));
    fields.insert("path".into(), Value::String(path.into()));
    if !protocol.is_empty() {
        fields.insert("protocol".into(), Value::String(protocol.into()));
    }
    fields.insert("status".into(), Value::String(status.into()));
    if size != "-" {
        fields.insert("size".into(), Value::String(size.into()));
    }

    // Derive level from status code
    let level = status.parse::<u16>().ok().and_then(|s| {
        if s >= 500 {
            Some(Level::Error)
        } else if s >= 400 {
            Some(Level::Warn)
        } else {
            Some(Level::Info)
        }
    });

    Some(Record {
        line_number,
        timestamp,
        level,
        message,
        fields,
        raw: line.to_string(),
    })
}

fn parse_combined_line(line: &str, line_number: usize) -> Option<Record> {
    let caps = COMBINED_RE.captures(line.trim())?;

    let remote_host = caps.get(1).map(|m| m.as_str()).unwrap_or("-");
    let ident = caps.get(2).map(|m| m.as_str()).unwrap_or("-");
    let auth_user = caps.get(3).map(|m| m.as_str()).unwrap_or("-");
    let ts_str = caps.get(4).map(|m| m.as_str()).unwrap_or("");
    let method = caps.get(5).map(|m| m.as_str()).unwrap_or("");
    let path = caps.get(6).map(|m| m.as_str()).unwrap_or("");
    let protocol = caps.get(7).map(|m| m.as_str()).unwrap_or("");
    let status = caps.get(8).map(|m| m.as_str()).unwrap_or("");
    let size = caps.get(9).map(|m| m.as_str()).unwrap_or("");
    let referer = caps.get(10).map(|m| m.as_str()).unwrap_or("-");
    let user_agent = caps.get(11).map(|m| m.as_str()).unwrap_or("-");

    let timestamp = parse_timestamp(ts_str);

    let message = format!("{} {} {}", method, path, protocol).trim().to_string();

    let mut fields = HashMap::new();
    fields.insert("remote_host".into(), Value::String(remote_host.into()));
    if ident != "-" {
        fields.insert("ident".into(), Value::String(ident.into()));
    }
    if auth_user != "-" {
        fields.insert("auth_user".into(), Value::String(auth_user.into()));
    }
    fields.insert("method".into(), Value::String(method.into()));
    fields.insert("path".into(), Value::String(path.into()));
    if !protocol.is_empty() {
        fields.insert("protocol".into(), Value::String(protocol.into()));
    }
    fields.insert("status".into(), Value::String(status.into()));
    if size != "-" {
        fields.insert("size".into(), Value::String(size.into()));
    }
    if referer != "-" {
        fields.insert("referer".into(), Value::String(referer.into()));
    }
    if user_agent != "-" {
        fields.insert("user_agent".into(), Value::String(user_agent.into()));
    }

    let level = status.parse::<u16>().ok().and_then(|s| {
        if s >= 500 {
            Some(Level::Error)
        } else if s >= 400 {
            Some(Level::Warn)
        } else {
            Some(Level::Info)
        }
    });

    Some(Record {
        line_number,
        timestamp,
        level,
        message,
        fields,
        raw: line.to_string(),
    })
}

fn parse_syslog_line(line: &str, line_number: usize) -> Option<Record> {
    let caps = SYSLOG_RE.captures(line.trim())?;

    let priority = caps.get(1).map(|m| m.as_str());
    let month = caps.get(2).map(|m| m.as_str()).unwrap_or("");
    let day = caps.get(3).map(|m| m.as_str()).unwrap_or("");
    let time = caps.get(4).map(|m| m.as_str()).unwrap_or("");
    let hostname = caps.get(5).map(|m| m.as_str()).unwrap_or("");
    let process = caps.get(6).map(|m| m.as_str()).unwrap_or("");
    let pid = caps.get(7).map(|m| m.as_str());
    let msg = caps.get(8).map(|m| m.as_str()).unwrap_or("");

    let ts_str = format!("{} {} {}", month, day.trim(), time);
    let timestamp = parse_timestamp(&ts_str);

    let mut fields = HashMap::new();
    fields.insert("hostname".into(), Value::String(hostname.into()));
    fields.insert("process".into(), Value::String(process.into()));
    if let Some(p) = pid {
        fields.insert("pid".into(), Value::String(p.into()));
    }
    if let Some(pri) = priority {
        fields.insert("priority".into(), Value::String(pri.into()));
        // Derive severity from priority (priority = facility * 8 + severity)
        if let Ok(pri_num) = pri.parse::<u8>() {
            let severity = pri_num & 0x07;
            let level = match severity {
                0 => Some(Level::Fatal),    // Emergency
                1 => Some(Level::Fatal),    // Alert
                2 => Some(Level::Fatal),    // Critical
                3 => Some(Level::Error),    // Error
                4 => Some(Level::Warn),     // Warning
                5 => Some(Level::Info),     // Notice
                6 => Some(Level::Info),     // Informational
                7 => Some(Level::Debug),    // Debug
                _ => None,
            };
            if let Some(l) = level {
                return Some(Record {
                    line_number,
                    timestamp,
                    level: Some(l),
                    message: msg.to_string(),
                    fields,
                    raw: line.to_string(),
                });
            }
        }
    }

    Some(Record {
        line_number,
        timestamp,
        level: None,
        message: msg.to_string(),
        fields,
        raw: line.to_string(),
    })
}

fn parse_plain_line(line: &str, line_number: usize) -> Option<Record> {
    if line.trim().is_empty() {
        return None;
    }

    // Try to extract a level
    let level = PLAIN_BRACKET_LEVEL_RE
        .captures(line)
        .or_else(|| PLAIN_LEVEL_RE.captures(line))
        .and_then(|caps| Level::from_str_loose(caps.get(1)?.as_str()));

    // Try to extract a leading timestamp
    let timestamp = PLAIN_TS_RE
        .captures(line)
        .and_then(|caps| parse_timestamp(caps.get(1)?.as_str()));

    Some(Record {
        line_number,
        timestamp,
        level,
        message: line.to_string(),
        fields: HashMap::new(),
        raw: line.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a single line according to the given format.
pub fn parse_line(format: Format, line: &str, line_number: usize) -> Option<Record> {
    match format {
        Format::JsonLines => parse_json_line(line, line_number),
        Format::Logfmt => parse_logfmt_line(line, line_number),
        Format::CommonLog => parse_clf_line(line, line_number),
        Format::CombinedLog => parse_combined_line(line, line_number),
        Format::Syslog => parse_syslog_line(line, line_number),
        Format::Plain => parse_plain_line(line, line_number),
    }
}

/// Detect the format from the input and parse all lines into records.
pub fn parse_input(input: &str) -> (Format, Vec<Record>) {
    let lines: Vec<&str> = input.lines().collect();

    let sample: Vec<&str> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .take(10)
        .copied()
        .collect();

    let format = detect(&sample);

    let records: Vec<Record> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| parse_line(format, line, i + 1))
        .collect();

    (format, records)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_json_lines() {
        let lines = vec![
            r#"{"timestamp":"2024-01-01T00:00:00Z","level":"info","message":"hello"}"#,
            r#"{"timestamp":"2024-01-01T00:00:01Z","level":"error","message":"oops"}"#,
        ];
        assert_eq!(detect(&lines), Format::JsonLines);
    }

    #[test]
    fn detect_logfmt() {
        let lines = vec![
            r#"ts=2024-01-01T00:00:00Z level=info msg="starting up" component=server"#,
            r#"ts=2024-01-01T00:00:01Z level=error msg="connection failed" err=timeout"#,
        ];
        assert_eq!(detect(&lines), Format::Logfmt);
    }

    #[test]
    fn detect_clf() {
        let lines = vec![
            r#"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326"#,
        ];
        assert_eq!(detect(&lines), Format::CommonLog);
    }

    #[test]
    fn detect_combined() {
        let lines = vec![
            r#"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /index.html HTTP/1.0" 200 2326 "http://www.example.com/start.html" "Mozilla/5.0""#,
        ];
        assert_eq!(detect(&lines), Format::CombinedLog);
    }

    #[test]
    fn detect_syslog() {
        let lines = vec![
            r#"<34>Jan  5 14:32:01 myhost sshd[1234]: Accepted publickey for user"#,
            r#"<34>Jan  5 14:32:02 myhost sshd[1234]: pam_unix(sshd:session): session opened"#,
        ];
        assert_eq!(detect(&lines), Format::Syslog);
    }

    #[test]
    fn detect_plain() {
        let lines = vec!["just some text", "more text here"];
        assert_eq!(detect(&lines), Format::Plain);
    }

    #[test]
    fn parse_json_record() {
        let line = r#"{"timestamp":"2024-01-15T10:30:00Z","level":"error","message":"disk full","host":"srv1"}"#;
        let rec = parse_json_line(line, 1).unwrap();
        assert!(rec.timestamp.is_some());
        assert_eq!(rec.level, Some(Level::Error));
        assert_eq!(rec.message, "disk full");
        assert_eq!(
            rec.fields.get("host"),
            Some(&Value::String("srv1".into()))
        );
    }

    #[test]
    fn parse_logfmt_record() {
        let line = r#"ts=2024-01-15T10:30:00Z level=warn msg="high latency" duration=1.5s"#;
        let rec = parse_logfmt_line(line, 1).unwrap();
        assert!(rec.timestamp.is_some());
        assert_eq!(rec.level, Some(Level::Warn));
        assert_eq!(rec.message, "high latency");
        assert!(rec.fields.contains_key("duration"));
    }

    #[test]
    fn parse_clf_record() {
        let line = r#"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326"#;
        let rec = parse_clf_line(line, 1).unwrap();
        assert!(rec.timestamp.is_some());
        assert_eq!(rec.level, Some(Level::Info));
        assert_eq!(
            rec.fields.get("method"),
            Some(&Value::String("GET".into()))
        );
        assert_eq!(
            rec.fields.get("path"),
            Some(&Value::String("/apache_pb.gif".into()))
        );
    }

    #[test]
    fn parse_combined_record() {
        let line = r#"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /index.html HTTP/1.0" 200 2326 "http://www.example.com/start.html" "Mozilla/5.0""#;
        let rec = parse_combined_line(line, 1).unwrap();
        assert!(rec.timestamp.is_some());
        assert!(rec.fields.contains_key("referer"));
        assert!(rec.fields.contains_key("user_agent"));
    }

    #[test]
    fn parse_syslog_record() {
        let line = r#"<34>Jan  5 14:32:01 myhost sshd[1234]: Accepted publickey for user"#;
        let rec = parse_syslog_line(line, 1).unwrap();
        assert!(rec.timestamp.is_some());
        assert_eq!(rec.message, "Accepted publickey for user");
        assert_eq!(
            rec.fields.get("hostname"),
            Some(&Value::String("myhost".into()))
        );
        assert_eq!(
            rec.fields.get("pid"),
            Some(&Value::String("1234".into()))
        );
    }

    #[test]
    fn parse_plain_with_level() {
        let line = "2024-01-15T10:30:00Z [ERROR] something went wrong";
        let rec = parse_plain_line(line, 1).unwrap();
        assert_eq!(rec.level, Some(Level::Error));
        assert!(rec.timestamp.is_some());
    }

    #[test]
    fn parse_input_end_to_end() {
        let input = r#"{"timestamp":"2024-01-15T10:30:00Z","level":"info","message":"hello"}
{"timestamp":"2024-01-15T10:30:01Z","level":"error","message":"oops"}"#;
        let (fmt, records) = parse_input(input);
        assert_eq!(fmt, Format::JsonLines);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].level, Some(Level::Info));
        assert_eq!(records[1].level, Some(Level::Error));
    }

    #[test]
    fn timestamp_unix_epoch() {
        let ts = parse_timestamp("1705312200");
        assert!(ts.is_some());
    }

    #[test]
    fn timestamp_unix_millis() {
        let ts = parse_timestamp("1705312200000");
        assert!(ts.is_some());
    }

    #[test]
    fn empty_input() {
        let (fmt, records) = parse_input("");
        assert_eq!(fmt, Format::Plain);
        assert!(records.is_empty());
    }
}
