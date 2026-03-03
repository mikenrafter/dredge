use crate::{Format, Level, Record};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Normalization regexes (compiled once)
// ---------------------------------------------------------------------------

static RE_UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .unwrap()
});

static RE_EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap()
});

static RE_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"https?://[^\s,;'")\]}>]+"#).unwrap()
});

static RE_IP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?\b").unwrap()
});

static RE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(/[a-zA-Z0-9._-]+){2,}").unwrap()
});

static RE_HEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-fA-F]{8,}\b").unwrap()
});

static RE_QUOTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"("[^"]*"|'[^']*')"#).unwrap()
});

static RE_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d+\b").unwrap()
});

// ---------------------------------------------------------------------------
// Error clustering
// ---------------------------------------------------------------------------

/// Replace variable parts of a log message with placeholders so that
/// structurally-similar messages group together.
pub fn normalize_message(msg: &str) -> String {
    let s = RE_UUID.replace_all(msg, "<UUID>");
    let s = RE_EMAIL.replace_all(&s, "<EMAIL>");
    let s = RE_URL.replace_all(&s, "<URL>");
    let s = RE_IP.replace_all(&s, "<IP>");
    let s = RE_PATH.replace_all(&s, "<PATH>");
    let s = RE_HEX.replace_all(&s, "<HEX>");
    let s = RE_QUOTED.replace_all(&s, "<STR>");
    let s = RE_NUMBER.replace_all(&s, "<N>");
    s.into_owned()
}

#[derive(Debug, Clone)]
pub struct ErrorCluster {
    pub normalized: String,
    pub count: usize,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub example: String,
}

/// Group records that have error-level or above by their normalized message.
/// Returns clusters sorted by count descending.
pub fn cluster_errors(records: &[Record]) -> Vec<ErrorCluster> {
    let mut map: HashMap<String, ErrorCluster> = HashMap::new();

    for r in records {
        let is_error = r
            .level
            .as_ref()
            .map_or(false, |l| l.is_error_or_above());
        if !is_error {
            continue;
        }

        let key = normalize_message(&r.message);
        let entry = map.entry(key.clone()).or_insert_with(|| ErrorCluster {
            normalized: key,
            count: 0,
            first_seen: None,
            last_seen: None,
            example: r.message.clone(),
        });

        entry.count += 1;

        if let Some(ts) = r.timestamp {
            entry.first_seen = Some(match entry.first_seen {
                Some(prev) => prev.min(ts),
                None => ts,
            });
            entry.last_seen = Some(match entry.last_seen {
                Some(prev) => prev.max(ts),
                None => ts,
            });
        }
    }

    let mut clusters: Vec<ErrorCluster> = map.into_values().collect();
    clusters.sort_by(|a, b| b.count.cmp(&a.count));
    clusters
}

// ---------------------------------------------------------------------------
// Time analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TimeWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone)]
pub enum Trend {
    IncreasingErrors { factor: f64 },
    Spike { window_start: DateTime<Utc>, count: usize, average: f64 },
    QuietPeriod { window_start: DateTime<Utc>, duration: Duration },
}

/// Bucket records into fixed-size time windows.
/// Only records with timestamps are included. Returns windows sorted by start time.
pub fn bucket_by_time(records: &[Record], bucket_size: Duration) -> Vec<TimeWindow> {
    let timestamps: Vec<_> = records
        .iter()
        .filter_map(|r| r.timestamp.map(|ts| (ts, r)))
        .collect();

    if timestamps.is_empty() {
        return Vec::new();
    }

    let min_ts = timestamps.iter().map(|(ts, _)| *ts).min().unwrap();
    let max_ts = timestamps.iter().map(|(ts, _)| *ts).max().unwrap();

    let bucket_millis = bucket_size.num_milliseconds();
    if bucket_millis <= 0 {
        return Vec::new();
    }

    // Build empty windows spanning the full range
    let mut windows: Vec<TimeWindow> = Vec::new();
    let mut cursor = min_ts;
    while cursor <= max_ts {
        let end = cursor + bucket_size;
        windows.push(TimeWindow {
            start: cursor,
            end,
            count: 0,
            error_count: 0,
        });
        cursor = end;
    }

    // Fill the windows
    for (ts, record) in &timestamps {
        let offset = ts.signed_duration_since(min_ts).num_milliseconds();
        let idx = (offset / bucket_millis) as usize;
        let idx = idx.min(windows.len() - 1);
        windows[idx].count += 1;
        if record
            .level
            .as_ref()
            .map_or(false, |l| l.is_error_or_above())
        {
            windows[idx].error_count += 1;
        }
    }

    windows
}

/// Detect trends across time windows.
pub fn detect_trends(windows: &[TimeWindow]) -> Vec<Trend> {
    let mut trends = Vec::new();

    if windows.len() < 2 {
        return trends;
    }

    // Average error count across all windows
    let total_errors: usize = windows.iter().map(|w| w.error_count).sum();
    let avg_errors = total_errors as f64 / windows.len() as f64;

    // Increasing error rate: compare first 3 vs last 3
    if windows.len() >= 6 {
        let first3_avg = windows[..3].iter().map(|w| w.error_count).sum::<usize>() as f64 / 3.0;
        let last3_avg = windows[windows.len() - 3..]
            .iter()
            .map(|w| w.error_count)
            .sum::<usize>() as f64
            / 3.0;

        if first3_avg > 0.0 && last3_avg > first3_avg {
            let factor = last3_avg / first3_avg;
            trends.push(Trend::IncreasingErrors { factor });
        }
    }

    // Spike: any window with error_count > 3x average
    if avg_errors > 0.0 {
        for w in windows {
            if w.error_count as f64 > 3.0 * avg_errors {
                trends.push(Trend::Spike {
                    window_start: w.start,
                    count: w.error_count,
                    average: avg_errors,
                });
            }
        }
    }

    // Quiet period: window with 0 records where at least one neighbor has records
    for i in 0..windows.len() {
        if windows[i].count == 0 {
            let prev_has = i > 0 && windows[i - 1].count > 0;
            let next_has = i + 1 < windows.len() && windows[i + 1].count > 0;
            if prev_has || next_has {
                let duration = windows[i]
                    .end
                    .signed_duration_since(windows[i].start);
                trends.push(Trend::QuietPeriod {
                    window_start: windows[i].start,
                    duration,
                });
            }
        }
    }

    trends
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Summary {
    pub total_records: usize,
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub level_counts: Vec<(Level, usize)>,
    pub format: Format,
    pub records_per_second: Option<f64>,
    pub error_clusters: Vec<ErrorCluster>,
    pub trends: Vec<Trend>,
    pub top_fields: Vec<(String, Vec<(String, usize)>)>,
}

/// Fields that are commonly interesting for log analysis.
const INTERESTING_FIELDS: &[&str] = &[
    "service", "host", "path", "method", "status", "source",
];

/// Build a full summary from a set of parsed records.
pub fn generate_summary(records: &[Record], format: Format) -> Summary {
    let total_records = records.len();

    // Time range
    let timestamps: Vec<DateTime<Utc>> = records.iter().filter_map(|r| r.timestamp).collect();
    let time_range = if timestamps.is_empty() {
        None
    } else {
        let min = *timestamps.iter().min().unwrap();
        let max = *timestamps.iter().max().unwrap();
        Some((min, max))
    };

    // Records per second
    let records_per_second = time_range.and_then(|(start, end)| {
        let secs = end.signed_duration_since(start).num_milliseconds() as f64 / 1000.0;
        if secs > 0.0 {
            Some(total_records as f64 / secs)
        } else {
            None
        }
    });

    // Level counts
    let mut level_map: HashMap<Level, usize> = HashMap::new();
    for r in records {
        if let Some(level) = r.level {
            *level_map.entry(level).or_insert(0) += 1;
        }
    }
    let mut level_counts: Vec<(Level, usize)> = level_map.into_iter().collect();
    level_counts.sort_by_key(|(level, _)| *level);

    // Error clusters
    let error_clusters = cluster_errors(records);

    // Time trends — auto-pick a reasonable bucket size
    let trends = if let Some((start, end)) = time_range {
        let span = end.signed_duration_since(start);
        let bucket_size = if span.num_hours() >= 24 {
            Duration::hours(1)
        } else if span.num_minutes() >= 60 {
            Duration::minutes(5)
        } else if span.num_minutes() >= 10 {
            Duration::minutes(1)
        } else {
            Duration::seconds(10)
        };
        let windows = bucket_by_time(records, bucket_size);
        detect_trends(&windows)
    } else {
        Vec::new()
    };

    // Top fields
    let mut top_fields: Vec<(String, Vec<(String, usize)>)> = Vec::new();
    for &field_name in INTERESTING_FIELDS {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for r in records {
            if let Some(val) = r.fields.get(field_name) {
                let s = match val {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                *counts.entry(s).or_insert(0) += 1;
            }
        }
        if !counts.is_empty() {
            let mut pairs: Vec<(String, usize)> = counts.into_iter().collect();
            pairs.sort_by(|a, b| b.1.cmp(&a.1));
            pairs.truncate(5);
            top_fields.push((field_name.to_string(), pairs));
        }
    }

    Summary {
        total_records,
        time_range,
        level_counts,
        format,
        records_per_second,
        error_clusters,
        trends,
        top_fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_replaces_uuids() {
        let msg = "Failed for user 550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(normalize_message(msg), "Failed for user <UUID>");
    }

    #[test]
    fn normalize_replaces_ips() {
        let msg = "Connection from 192.168.1.1:8080 refused";
        assert_eq!(normalize_message(msg), "Connection from <IP> refused");
    }

    #[test]
    fn normalize_replaces_emails() {
        let msg = "Sent to user@example.com failed";
        assert_eq!(normalize_message(msg), "Sent to <EMAIL> failed");
    }

    #[test]
    fn normalize_replaces_urls() {
        let msg = "GET https://api.example.com/v1/users returned 500";
        assert_eq!(normalize_message(msg), "GET <URL> returned <N>");
    }

    #[test]
    fn normalize_replaces_quoted_strings() {
        let msg = r#"Key "some-key" not found"#;
        assert_eq!(normalize_message(msg), "Key <STR> not found");
    }

    #[test]
    fn normalize_replaces_numbers() {
        let msg = "Retry attempt 3 of 5";
        assert_eq!(normalize_message(msg), "Retry attempt <N> of <N>");
    }

    #[test]
    fn normalize_replaces_hex() {
        let msg = "Checksum mismatch: deadbeef01234567";
        assert_eq!(normalize_message(msg), "Checksum mismatch: <HEX>");
    }

    #[test]
    fn normalize_replaces_paths() {
        let msg = "File not found: /var/log/app/error.log";
        assert_eq!(normalize_message(msg), "File not found: <PATH>");
    }

    #[test]
    fn cluster_groups_similar_errors() {
        let records = vec![
            Record {
                line_number: 1,
                timestamp: None,
                level: Some(Level::Error),
                message: "Timeout connecting to 10.0.0.1:3306".into(),
                fields: HashMap::new(),
                raw: String::new(),
            },
            Record {
                line_number: 2,
                timestamp: None,
                level: Some(Level::Error),
                message: "Timeout connecting to 10.0.0.2:3306".into(),
                fields: HashMap::new(),
                raw: String::new(),
            },
            Record {
                line_number: 3,
                timestamp: None,
                level: Some(Level::Info),
                message: "All good".into(),
                fields: HashMap::new(),
                raw: String::new(),
            },
        ];

        let clusters = cluster_errors(&records);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count, 2);
        assert_eq!(clusters[0].normalized, "Timeout connecting to <IP>");
    }

    #[test]
    fn empty_records_produce_empty_summary() {
        let summary = generate_summary(&[], Format::JsonLines);
        assert_eq!(summary.total_records, 0);
        assert!(summary.time_range.is_none());
        assert!(summary.records_per_second.is_none());
        assert!(summary.error_clusters.is_empty());
        assert!(summary.trends.is_empty());
    }
}
