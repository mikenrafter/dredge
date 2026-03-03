use chrono::{DateTime, Utc};
use colored::Colorize;
use serde_json::{self, json, Value};
use std::io::{self, Write};

use crate::analysis::{Summary, Trend};
use crate::{Level, Record};

// ── Constants ────────────────────────────────────────────────────────

const BAR_WIDTH: usize = 20;
const BOX_WIDTH: usize = 48;

// ── Helpers ──────────────────────────────────────────────────────────

/// Format a chrono::Duration into a human-readable string like "2h 15m" or "3d 4h".
pub fn format_duration(d: chrono::Duration) -> String {
    let total_secs = d.num_seconds().unsigned_abs();
    if total_secs == 0 {
        return "0s".to_string();
    }
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if days > 0 {
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    } else if hours > 0 {
        if minutes > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}h", hours)
        }
    } else if minutes > 0 {
        if seconds > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}m", minutes)
        }
    } else {
        format!("{}s", seconds)
    }
}

/// Format a DateTime relative to now, like "5m ago" or "2h ago".
pub fn format_relative(dt: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(dt);
    if diff.num_seconds() < 0 {
        return "in the future".to_string();
    }
    format!("{} ago", format_duration(diff))
}

fn color_level(level: Level) -> colored::ColoredString {
    let s = level.as_str();
    match level {
        Level::Trace => s.dimmed(),
        Level::Debug => s.blue(),
        Level::Info => s.green(),
        Level::Warn => s.yellow(),
        Level::Error => s.red(),
        Level::Fatal => s.red().bold(),
    }
}

/// Pad a level string to fixed width (5 chars) for alignment.
fn pad_level(level: Level) -> String {
    format!("{:<5}", level.as_str())
}

fn format_field_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn bar_chart(fraction: f64, width: usize) -> String {
    let filled = ((fraction * width as f64).round() as usize).min(width);
    let empty = width - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn format_number(n: usize) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

// ── Record printing ──────────────────────────────────────────────────

/// Print a single log record with colors.
///
/// Compact mode: single line with timestamp, level, message, and inline fields.
/// Verbose mode: message on first line, fields on indented lines below.
pub fn print_record(record: &Record, verbose: bool) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    write_record(&mut out, record, verbose);
}

fn write_record(out: &mut impl Write, record: &Record, verbose: bool) {
    // Timestamp
    let ts = match &record.timestamp {
        Some(dt) => dt.format("%Y-%m-%dT%H:%M:%SZ").to_string().dimmed().to_string(),
        None => String::new(),
    };

    // Level
    let lvl = match record.level {
        Some(l) => {
            let padded = pad_level(l);
            match l {
                Level::Trace => padded.dimmed().to_string(),
                Level::Debug => padded.blue().to_string(),
                Level::Info => padded.green().to_string(),
                Level::Warn => padded.yellow().to_string(),
                Level::Error => padded.red().to_string(),
                Level::Fatal => padded.red().bold().to_string(),
            }
        }
        None => "     ".to_string(),
    };

    // Message
    let msg = &record.message;

    if verbose {
        // Verbose: message on first line, fields below
        let _ = writeln!(out, "{} {} {}", ts, lvl, msg.white().bold());
        let mut keys: Vec<_> = record.fields.keys().collect();
        keys.sort();
        for key in keys {
            let val = format_field_value(&record.fields[key]);
            let _ = writeln!(out, "    {} {}", format!("{}:", key).cyan(), val);
        }
    } else {
        // Compact: everything on one line
        let mut line = format!("{} {} {}", ts, lvl, msg);
        let mut keys: Vec<_> = record.fields.keys().collect();
        keys.sort();
        for key in &keys {
            let val = format_field_value(&record.fields[*key]);
            line.push_str(&format!(" {}", format!("{}={}", key, val).cyan().to_string()));
        }
        let _ = writeln!(out, "{}", line);
    }
}

/// Print multiple records.
pub fn print_records(records: &[Record], verbose: bool) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for record in records {
        write_record(&mut out, record, verbose);
    }
}

// ── Summary printing ─────────────────────────────────────────────────

/// Print the full analysis summary with box-drawing and color.
pub fn print_summary(summary: &Summary) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    write_summary(&mut out, summary).ok();
}

fn write_summary(out: &mut impl Write, summary: &Summary) -> io::Result<()> {
    writeln!(out)?;

    // ── Header box ───────────────────────────────────────────────
    let total = format_number(summary.total_records);
    let span = match &summary.time_range {
        Some((start, end)) => {
            let d = end.signed_duration_since(*start);
            format_duration(d)
        }
        None => "—".to_string(),
    };
    let rate = match summary.records_per_second {
        Some(r) => format!("{:.1} rec/sec", r),
        None => "—".to_string(),
    };
    let format_str = format!("Format: {}", summary.format);

    let header_line = format!("{} records | {} span | {}", total, span, rate);

    // Use char count for visual width calculations
    let header_chars = header_line.chars().count();
    let format_chars = format_str.chars().count();
    let content_width = BOX_WIDTH.max(header_chars).max(format_chars);

    let title = " Summary ";
    let title_chars = title.chars().count();
    let top_rule = "─".repeat(content_width + 2 - title_chars - 1);
    writeln!(out, "{}{}{}{}", "╭─".dimmed(), title.bold(), top_rule.dimmed(), "╮".dimmed())?;

    let header_pad = " ".repeat(content_width - header_chars);
    writeln!(out, "{} {}{} {}", "│".dimmed(), header_line, header_pad, "│".dimmed())?;

    let format_pad = " ".repeat(content_width - format_chars);
    let format_line = format!("{}{}", format_str, format_pad);
    writeln!(out, "{} {} {}", "│".dimmed(), format_line.dimmed(), "│".dimmed())?;

    writeln!(out, "{}{}{}", "╰".dimmed(), "─".repeat(content_width + 2).dimmed(), "╯".dimmed())?;

    writeln!(out)?;

    // ── Levels ───────────────────────────────────────────────────
    if !summary.level_counts.is_empty() {
        writeln!(out, "{}", "  Levels".bold())?;

        let max_count = summary.level_counts.iter().map(|(_, c)| *c).max().unwrap_or(1);
        let total_f = summary.total_records as f64;

        for (level, count) in &summary.level_counts {
            let pct = if total_f > 0.0 {
                *count as f64 / total_f * 100.0
            } else {
                0.0
            };
            let fraction = *count as f64 / max_count as f64;
            let bar = bar_chart(fraction, BAR_WIDTH);
            let count_str = format_number(*count);

            writeln!(
                out,
                "    {} {:>8}  {}  {:>5.1}%",
                color_level(*level),
                count_str,
                bar.dimmed(),
                pct,
            )?;
        }
        writeln!(out)?;
    }

    // ── Top Errors ───────────────────────────────────────────────
    if !summary.error_clusters.is_empty() {
        writeln!(out, "{}", "  Top Errors".bold())?;

        for (i, cluster) in summary.error_clusters.iter().enumerate() {
            let count_str = format!("{}×", format_number(cluster.count));
            let first = cluster
                .first_seen
                .map(|dt| format!("first {}", format_relative(dt)))
                .unwrap_or_default();
            let last = cluster
                .last_seen
                .map(|dt| format!("last {}", format_relative(dt)))
                .unwrap_or_default();

            let mut meta_parts = vec![count_str];
            if !first.is_empty() {
                meta_parts.push(first);
            }
            if !last.is_empty() {
                meta_parts.push(last);
            }
            let meta = meta_parts.join(" │ ");

            writeln!(
                out,
                "    {}. {} ({})",
                format!("{}", i + 1).dimmed(),
                cluster.normalized.white(),
                meta.dimmed(),
            )?;
        }
        writeln!(out)?;
    }

    // ── Trends ───────────────────────────────────────────────────
    if !summary.trends.is_empty() {
        writeln!(out, "{}", "  Trends".bold())?;

        for trend in &summary.trends {
            match trend {
                Trend::IncreasingErrors { factor } => {
                    writeln!(
                        out,
                        "    {} Error rate increasing {:.1}× over recent window",
                        "⚠".yellow(),
                        factor,
                    )?;
                }
                Trend::Spike {
                    window_start,
                    count,
                    average,
                } => {
                    writeln!(
                        out,
                        "    {} Spike at {} ({} errors vs avg {:.0})",
                        "⚡".red(),
                        window_start.format("%H:%M UTC"),
                        count,
                        average,
                    )?;
                }
                Trend::QuietPeriod {
                    window_start,
                    duration,
                } => {
                    writeln!(
                        out,
                        "    {} Quiet period at {} ({})",
                        "◇".dimmed(),
                        window_start.format("%H:%M UTC"),
                        format_duration(*duration),
                    )?;
                }
            }
        }
        writeln!(out)?;
    }

    // ── Top Fields ───────────────────────────────────────────────
    if !summary.top_fields.is_empty() {
        writeln!(out, "{}", "  Top Fields".bold())?;

        for (field_name, values) in &summary.top_fields {
            let parts: Vec<String> = values
                .iter()
                .take(5)
                .map(|(val, count)| {
                    let pct = if summary.total_records > 0 {
                        *count as f64 / summary.total_records as f64 * 100.0
                    } else {
                        0.0
                    };
                    format!("{} ({:.0}%)", val, pct)
                })
                .collect();
            writeln!(
                out,
                "    {}: {}",
                field_name.cyan(),
                parts.join(", "),
            )?;
        }
        writeln!(out)?;
    }

    Ok(())
}

// ── Aggregation ──────────────────────────────────────────────────────

/// Print count-by aggregation results as a table with bars.
pub fn print_aggregation(results: &[(String, usize)], field_name: &str) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    write_aggregation(&mut out, results, field_name).ok();
}

fn write_aggregation(
    out: &mut impl Write,
    results: &[(String, usize)],
    field_name: &str,
) -> io::Result<()> {
    if results.is_empty() {
        writeln!(out, "{}", "  No results.".dimmed())?;
        return Ok(());
    }

    let total: usize = results.iter().map(|(_, c)| *c).sum();
    let max_count = results.iter().map(|(_, c)| *c).max().unwrap_or(1);
    let max_label_len = results.iter().map(|(k, _)| k.len()).max().unwrap_or(0);

    writeln!(out)?;
    writeln!(out, "  {} {}", "count-by".dimmed(), field_name.cyan().bold())?;
    writeln!(out)?;

    for (value, count) in results {
        let fraction = *count as f64 / max_count as f64;
        let bar = bar_chart(fraction, BAR_WIDTH);
        let pct = if total > 0 {
            *count as f64 / total as f64 * 100.0
        } else {
            0.0
        };

        writeln!(
            out,
            "    {:<width$}  {:>8}  {}  {:>5.1}%",
            value.white(),
            format_number(*count),
            bar.dimmed(),
            pct,
            width = max_label_len,
        )?;
    }
    writeln!(out)?;

    Ok(())
}

/// Print count-by aggregation as JSON.
pub fn print_json_aggregation(results: &[(String, usize)], field_name: &str) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let entries: Vec<Value> = results
        .iter()
        .map(|(v, c)| json!({ "value": v, "count": c }))
        .collect();
    let obj = json!({ "field": field_name, "results": entries });
    let _ = writeln!(out, "{}", serde_json::to_string_pretty(&obj).unwrap());
}

// ── JSON output ──────────────────────────────────────────────────────

/// Print records as JSON lines (one JSON object per line).
pub fn print_json_records(records: &[Record]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for record in records {
        let mut obj = serde_json::Map::new();
        if let Some(ts) = &record.timestamp {
            obj.insert("timestamp".into(), json!(ts.to_rfc3339()));
        }
        if let Some(level) = &record.level {
            obj.insert("level".into(), json!(level.as_str()));
        }
        obj.insert("message".into(), json!(&record.message));
        obj.insert("line".into(), json!(record.line_number));
        for (k, v) in &record.fields {
            obj.insert(k.clone(), v.clone());
        }
        let _ = writeln!(out, "{}", Value::Object(obj));
    }
}

/// Print summary as pretty-printed JSON.
pub fn print_json_summary(summary: &Summary) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let time_range = summary.time_range.map(|(s, e)| {
        json!({
            "start": s.to_rfc3339(),
            "end": e.to_rfc3339(),
        })
    });

    let level_counts: serde_json::Map<String, Value> = summary
        .level_counts
        .iter()
        .map(|(l, c)| (l.as_str().to_string(), json!(c)))
        .collect();

    let error_clusters: Vec<Value> = summary
        .error_clusters
        .iter()
        .map(|c| {
            json!({
                "normalized": &c.normalized,
                "count": c.count,
                "example": &c.example,
                "first_seen": c.first_seen.map(|dt| dt.to_rfc3339()),
                "last_seen": c.last_seen.map(|dt| dt.to_rfc3339()),
            })
        })
        .collect();

    let trends: Vec<Value> = summary
        .trends
        .iter()
        .map(|t| match t {
            Trend::IncreasingErrors { factor } => {
                json!({"type": "increasing_errors", "factor": factor})
            }
            Trend::Spike {
                window_start,
                count,
                average,
            } => json!({
                "type": "spike",
                "window_start": window_start.to_rfc3339(),
                "count": count,
                "average": average,
            }),
            Trend::QuietPeriod {
                window_start,
                duration,
            } => json!({
                "type": "quiet_period",
                "window_start": window_start.to_rfc3339(),
                "duration_seconds": duration.num_seconds(),
            }),
        })
        .collect();

    let top_fields: serde_json::Map<String, Value> = summary
        .top_fields
        .iter()
        .map(|(name, values)| {
            let entries: Vec<Value> = values
                .iter()
                .map(|(v, c)| json!({"value": v, "count": c}))
                .collect();
            (name.clone(), json!(entries))
        })
        .collect();

    let obj = json!({
        "total_records": summary.total_records,
        "time_range": time_range,
        "format": format!("{}", summary.format),
        "records_per_second": summary.records_per_second,
        "level_counts": level_counts,
        "error_clusters": error_clusters,
        "trends": trends,
        "top_fields": top_fields,
    });

    let _ = writeln!(out, "{}", serde_json::to_string_pretty(&obj).unwrap());
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds() {
        let d = chrono::Duration::seconds(45);
        assert_eq!(format_duration(d), "45s");
    }

    #[test]
    fn test_format_duration_minutes() {
        let d = chrono::Duration::seconds(130);
        assert_eq!(format_duration(d), "2m 10s");
    }

    #[test]
    fn test_format_duration_hours() {
        let d = chrono::Duration::seconds(8100);
        assert_eq!(format_duration(d), "2h 15m");
    }

    #[test]
    fn test_format_duration_days() {
        let d = chrono::Duration::seconds(100800);
        assert_eq!(format_duration(d), "1d 4h");
    }

    #[test]
    fn test_format_duration_zero() {
        let d = chrono::Duration::seconds(0);
        assert_eq!(format_duration(d), "0s");
    }

    #[test]
    fn test_format_duration_exact_hour() {
        let d = chrono::Duration::seconds(3600);
        assert_eq!(format_duration(d), "1h");
    }

    #[test]
    fn test_format_number_small() {
        assert_eq!(format_number(42), "42");
    }

    #[test]
    fn test_format_number_thousands() {
        assert_eq!(format_number(25000), "25,000");
    }

    #[test]
    fn test_format_number_millions() {
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn test_bar_chart_full() {
        let bar = bar_chart(1.0, 10);
        assert_eq!(bar, "██████████");
    }

    #[test]
    fn test_bar_chart_half() {
        let bar = bar_chart(0.5, 10);
        assert_eq!(bar, "█████░░░░░");
    }

    #[test]
    fn test_bar_chart_empty() {
        let bar = bar_chart(0.0, 10);
        assert_eq!(bar, "░░░░░░░░░░");
    }

    #[test]
    fn test_write_record_compact() {
        let record = Record {
            line_number: 1,
            timestamp: Some("2026-03-03T01:23:45Z".parse().unwrap()),
            level: Some(Level::Error),
            message: "Connection timeout".to_string(),
            fields: {
                let mut m = std::collections::HashMap::new();
                m.insert("service".to_string(), json!("auth"));
                m
            },
            raw: String::new(),
        };
        let mut buf = Vec::new();
        write_record(&mut buf, &record, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Connection timeout"));
        assert!(output.contains("service=auth"));
    }

    #[test]
    fn test_write_record_verbose() {
        let record = Record {
            line_number: 1,
            timestamp: Some("2026-03-03T01:23:45Z".parse().unwrap()),
            level: Some(Level::Info),
            message: "Request handled".to_string(),
            fields: {
                let mut m = std::collections::HashMap::new();
                m.insert("method".to_string(), json!("GET"));
                m.insert("path".to_string(), json!("/api/health"));
                m
            },
            raw: String::new(),
        };
        let mut buf = Vec::new();
        write_record(&mut buf, &record, true);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Request handled"));
        assert!(output.contains("method:"));
        assert!(output.contains("path:"));
    }
}
