use crate::{Level, Record};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

/// Comparison operators for numeric field filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompOp {
    Gt,
    Lt,
    Gte,
    Lte,
    Ne,
    Eq,
}

/// A composable filter expression that can be evaluated against a Record.
#[derive(Debug, Clone)]
pub enum Filter {
    /// Exact match on a field value.
    FieldEquals { field: String, value: String },
    /// Substring match on a field value.
    FieldContains { field: String, value: String },
    /// Numeric comparison on a field value.
    FieldComparison { field: String, op: CompOp, value: f64 },
    /// Minimum log level (inclusive).
    LevelAtLeast(Level),
    /// Search across message and all field values.
    TextSearch(String),
    /// Regex search across message and all field values.
    RegexSearch(Regex),
    /// Records after this timestamp.
    TimeAfter(DateTime<Utc>),
    /// Records before this timestamp.
    TimeBefore(DateTime<Utc>),
    /// All sub-filters must match.
    And(Vec<Filter>),
    /// Any sub-filter must match.
    Or(Vec<Filter>),
    /// Negation.
    Not(Box<Filter>),
}

impl Filter {
    /// Check whether a record matches this filter.
    pub fn matches(&self, record: &Record) -> bool {
        match self {
            Filter::FieldEquals { field, value } => {
                let actual = resolve_field(record, field);
                actual.as_deref() == Some(value.as_str())
            }
            Filter::FieldContains { field, value } => {
                let actual = resolve_field(record, field);
                actual
                    .map(|a| a.to_lowercase().contains(&value.to_lowercase()))
                    .unwrap_or(false)
            }
            Filter::FieldComparison { field, op, value } => {
                let actual = resolve_field(record, field);
                match actual.and_then(|s| s.parse::<f64>().ok()) {
                    Some(n) => match op {
                        CompOp::Gt => n > *value,
                        CompOp::Lt => n < *value,
                        CompOp::Gte => n >= *value,
                        CompOp::Lte => n <= *value,
                        CompOp::Ne => (n - *value).abs() > f64::EPSILON,
                        CompOp::Eq => (n - *value).abs() < f64::EPSILON,
                    },
                    None => false,
                }
            }
            Filter::LevelAtLeast(min) => record.level.map(|l| l >= *min).unwrap_or(false),
            Filter::TextSearch(needle) => {
                let lower = needle.to_lowercase();
                if record.message.to_lowercase().contains(&lower) {
                    return true;
                }
                record.fields.values().any(|v| {
                    value_to_string(v).to_lowercase().contains(&lower)
                })
            }
            Filter::RegexSearch(re) => {
                if re.is_match(&record.message) {
                    return true;
                }
                record.fields.values().any(|v| re.is_match(&value_to_string(v)))
            }
            Filter::TimeAfter(after) => {
                record.timestamp.map(|t| t > *after).unwrap_or(false)
            }
            Filter::TimeBefore(before) => {
                record.timestamp.map(|t| t < *before).unwrap_or(false)
            }
            Filter::And(filters) => filters.iter().all(|f| f.matches(record)),
            Filter::Or(filters) => filters.iter().any(|f| f.matches(record)),
            Filter::Not(inner) => !inner.matches(record),
        }
    }
}

/// Resolve a field name against a record, supporting virtual fields.
fn resolve_field(record: &Record, field: &str) -> Option<String> {
    match field {
        "level" => record.level.map(|l| l.as_str().to_lowercase()),
        "message" | "msg" => Some(record.message.clone()),
        "timestamp" | "time" | "ts" => record.timestamp.map(|t| t.to_rfc3339()),
        _ => record.fields.get(field).map(|v| value_to_string(v)),
    }
}

/// Convert a serde_json::Value to a plain string for matching.
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Filter parsing
// ---------------------------------------------------------------------------

/// Parse a filter expression string into a Filter.
///
/// Supported formats:
/// - `field == "value"` or `field == value` → FieldEquals
/// - `field != value` → Not(FieldEquals)
/// - `field > 500`, `field >= 500`, `field < 500`, `field <= 500` → FieldComparison
/// - `field contains "value"` → FieldContains
/// - `level >= warn` → LevelAtLeast
/// - bare string → TextSearch
pub fn parse_filter(expr: &str) -> Result<Filter, String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err("empty filter expression".to_string());
    }

    // Try `field contains "value"`
    if let Some(filter) = try_parse_contains(expr) {
        return Ok(filter);
    }

    // Try `field op value` with comparison operators
    if let Some(filter) = try_parse_comparison(expr)? {
        return Ok(filter);
    }

    // Fall back to text search
    let search = unquote(expr);
    Ok(Filter::TextSearch(search.to_string()))
}

fn try_parse_contains(expr: &str) -> Option<Filter> {
    // Match: field contains "value" or field contains value
    let re = Regex::new(r#"(?i)^(\S+)\s+contains\s+(.+)$"#).ok()?;
    let caps = re.captures(expr)?;
    let field = caps.get(1)?.as_str().to_string();
    let value = unquote(caps.get(2)?.as_str().trim()).to_string();
    Some(Filter::FieldContains { field, value })
}

fn try_parse_comparison(expr: &str) -> Result<Option<Filter>, String> {
    // Match: field op value — operators: ==, !=, >=, <=, >, <
    let re = Regex::new(r#"^(\S+)\s*(==|!=|>=|<=|>|<)\s*(.+)$"#)
        .map_err(|e| e.to_string())?;
    let caps = match re.captures(expr) {
        Some(c) => c,
        None => return Ok(None),
    };

    let field = caps.get(1).unwrap().as_str();
    let op_str = caps.get(2).unwrap().as_str();
    let raw_value = caps.get(3).unwrap().as_str().trim();
    let value = unquote(raw_value);

    // Special handling for level field
    if field == "level" {
        if let Some(level) = Level::from_str_loose(&value) {
            return match op_str {
                ">=" => Ok(Some(Filter::LevelAtLeast(level))),
                "==" => Ok(Some(Filter::FieldEquals {
                    field: field.to_string(),
                    value: level.as_str().to_lowercase(),
                })),
                "!=" => Ok(Some(Filter::Not(Box::new(Filter::FieldEquals {
                    field: field.to_string(),
                    value: level.as_str().to_lowercase(),
                })))),
                _ => Err(format!("unsupported operator '{op_str}' for level field")),
            };
        }
    }

    // Try numeric comparison
    if let Ok(num) = value.parse::<f64>() {
        let op = match op_str {
            ">" => CompOp::Gt,
            "<" => CompOp::Lt,
            ">=" => CompOp::Gte,
            "<=" => CompOp::Lte,
            "!=" => CompOp::Ne,
            "==" => CompOp::Eq,
            _ => return Err(format!("unknown operator: {op_str}")),
        };
        return Ok(Some(Filter::FieldComparison {
            field: field.to_string(),
            op,
            value: num,
        }));
    }

    // String equality / inequality
    match op_str {
        "==" => Ok(Some(Filter::FieldEquals {
            field: field.to_string(),
            value: value.to_string(),
        })),
        "!=" => Ok(Some(Filter::Not(Box::new(Filter::FieldEquals {
            field: field.to_string(),
            value: value.to_string(),
        })))),
        _ => Err(format!(
            "operator '{op_str}' requires a numeric value, got '{value}'"
        )),
    }
}

/// Strip surrounding quotes from a string value.
fn unquote(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// Duration parsing
// ---------------------------------------------------------------------------

/// Parse a human-readable duration string into a chrono::Duration.
///
/// Supported units: `s` (seconds), `m` (minutes), `h` (hours), `d` (days), `w` (weeks).
/// Compound forms like `2h30m` and `1d12h` are supported.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration string".to_string());
    }

    let re = Regex::new(r"(\d+)\s*([smhdw])")
        .map_err(|e| e.to_string())?;

    let mut total_secs: i64 = 0;
    let mut matched = false;

    for cap in re.captures_iter(s) {
        matched = true;
        let amount: i64 = cap[1].parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
        let unit = &cap[2];
        let secs = match unit {
            "s" => amount,
            "m" => amount * 60,
            "h" => amount * 3600,
            "d" => amount * 86400,
            "w" => amount * 604800,
            _ => return Err(format!("unknown duration unit: {unit}")),
        };
        total_secs += secs;
    }

    if !matched {
        return Err(format!("invalid duration format: '{s}'"));
    }

    Duration::try_seconds(total_secs).ok_or_else(|| "duration out of range".to_string())
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// Group records by a field value and return counts sorted descending.
pub fn count_by_field(records: &[Record], field: &str) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for record in records {
        let key = resolve_field(record, field).unwrap_or_else(|| "<none>".to_string());
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut result: Vec<(String, usize)> = counts.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    result
}

/// Count records per log level, sorted by count descending.
pub fn count_by_level(records: &[Record]) -> Vec<(Level, usize)> {
    let mut counts: HashMap<Level, usize> = HashMap::new();
    for record in records {
        if let Some(level) = record.level {
            *counts.entry(level).or_insert(0) += 1;
        }
    }
    let mut result: Vec<(Level, usize)> = counts.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_record(message: &str, level: Option<Level>, fields: Vec<(&str, Value)>) -> Record {
        Record {
            line_number: 1,
            timestamp: None,
            level,
            message: message.to_string(),
            fields: fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            raw: String::new(),
        }
    }

    #[test]
    fn test_field_equals() {
        let r = make_record("hello", None, vec![("service", json!("auth"))]);
        let f = Filter::FieldEquals {
            field: "service".into(),
            value: "auth".into(),
        };
        assert!(f.matches(&r));

        let f2 = Filter::FieldEquals {
            field: "service".into(),
            value: "web".into(),
        };
        assert!(!f2.matches(&r));
    }

    #[test]
    fn test_field_contains() {
        let r = make_record("hello", None, vec![("service", json!("auth-service"))]);
        let f = Filter::FieldContains {
            field: "service".into(),
            value: "auth".into(),
        };
        assert!(f.matches(&r));
    }

    #[test]
    fn test_field_comparison() {
        let r = make_record("", None, vec![("status", json!(502))]);
        let f = Filter::FieldComparison {
            field: "status".into(),
            op: CompOp::Gte,
            value: 500.0,
        };
        assert!(f.matches(&r));

        let f2 = Filter::FieldComparison {
            field: "status".into(),
            op: CompOp::Lt,
            value: 500.0,
        };
        assert!(!f2.matches(&r));
    }

    #[test]
    fn test_level_at_least() {
        let r = make_record("", Some(Level::Error), vec![]);
        assert!(Filter::LevelAtLeast(Level::Warn).matches(&r));
        assert!(Filter::LevelAtLeast(Level::Error).matches(&r));
        assert!(!Filter::LevelAtLeast(Level::Fatal).matches(&r));
    }

    #[test]
    fn test_text_search() {
        let r = make_record("connection timeout", None, vec![("host", json!("db-1"))]);
        assert!(Filter::TextSearch("timeout".into()).matches(&r));
        assert!(Filter::TextSearch("db-1".into()).matches(&r));
        assert!(!Filter::TextSearch("missing".into()).matches(&r));
    }

    #[test]
    fn test_regex_search() {
        let r = make_record("error code 503", None, vec![]);
        let re = Regex::new(r"code \d{3}").unwrap();
        assert!(Filter::RegexSearch(re).matches(&r));
    }

    #[test]
    fn test_and_or_not() {
        let r = make_record("error", Some(Level::Error), vec![("status", json!(500))]);
        let f = Filter::And(vec![
            Filter::LevelAtLeast(Level::Error),
            Filter::FieldComparison {
                field: "status".into(),
                op: CompOp::Eq,
                value: 500.0,
            },
        ]);
        assert!(f.matches(&r));

        let f_or = Filter::Or(vec![
            Filter::TextSearch("missing".into()),
            Filter::TextSearch("error".into()),
        ]);
        assert!(f_or.matches(&r));

        let f_not = Filter::Not(Box::new(Filter::LevelAtLeast(Level::Fatal)));
        assert!(f_not.matches(&r));
    }

    #[test]
    fn test_virtual_field_level() {
        let r = make_record("test", Some(Level::Error), vec![]);
        let f = Filter::FieldEquals {
            field: "level".into(),
            value: "error".into(),
        };
        assert!(f.matches(&r));
    }

    #[test]
    fn test_virtual_field_message() {
        let r = make_record("hello world", None, vec![]);
        let f = Filter::FieldContains {
            field: "message".into(),
            value: "world".into(),
        };
        assert!(f.matches(&r));
    }

    #[test]
    fn test_parse_filter_equals() {
        let f = parse_filter(r#"service == "auth""#).unwrap();
        let r = make_record("", None, vec![("service", json!("auth"))]);
        assert!(f.matches(&r));
    }

    #[test]
    fn test_parse_filter_comparison() {
        let f = parse_filter("status >= 500").unwrap();
        let r = make_record("", None, vec![("status", json!(503))]);
        assert!(f.matches(&r));
    }

    #[test]
    fn test_parse_filter_contains() {
        let f = parse_filter(r#"service contains "auth""#).unwrap();
        let r = make_record("", None, vec![("service", json!("auth-service"))]);
        assert!(f.matches(&r));
    }

    #[test]
    fn test_parse_filter_level() {
        let f = parse_filter("level >= warn").unwrap();
        let r = make_record("", Some(Level::Error), vec![]);
        assert!(f.matches(&r));
        let r2 = make_record("", Some(Level::Debug), vec![]);
        assert!(!f.matches(&r2));
    }

    #[test]
    fn test_parse_filter_text_search() {
        let f = parse_filter("timeout").unwrap();
        let r = make_record("connection timeout", None, vec![]);
        assert!(f.matches(&r));
    }

    #[test]
    fn test_parse_duration_simple() {
        assert_eq!(parse_duration("5m").unwrap(), Duration::try_minutes(5).unwrap());
        assert_eq!(parse_duration("1h").unwrap(), Duration::try_hours(1).unwrap());
        assert_eq!(parse_duration("30s").unwrap(), Duration::try_seconds(30).unwrap());
        assert_eq!(parse_duration("1d").unwrap(), Duration::try_days(1).unwrap());
        assert_eq!(parse_duration("1w").unwrap(), Duration::try_weeks(1).unwrap());
    }

    #[test]
    fn test_parse_duration_compound() {
        assert_eq!(
            parse_duration("2h30m").unwrap(),
            Duration::try_seconds(2 * 3600 + 30 * 60).unwrap()
        );
        assert_eq!(
            parse_duration("1d12h").unwrap(),
            Duration::try_seconds(86400 + 12 * 3600).unwrap()
        );
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn test_count_by_field() {
        let records = vec![
            make_record("", None, vec![("service", json!("auth"))]),
            make_record("", None, vec![("service", json!("auth"))]),
            make_record("", None, vec![("service", json!("web"))]),
        ];
        let counts = count_by_field(&records, "service");
        assert_eq!(counts[0], ("auth".to_string(), 2));
        assert_eq!(counts[1], ("web".to_string(), 1));
    }

    #[test]
    fn test_count_by_level() {
        let records = vec![
            make_record("", Some(Level::Error), vec![]),
            make_record("", Some(Level::Error), vec![]),
            make_record("", Some(Level::Info), vec![]),
            make_record("", None, vec![]),
        ];
        let counts = count_by_level(&records);
        assert_eq!(counts[0], (Level::Error, 2));
        assert_eq!(counts[1], (Level::Info, 1));
    }
}
