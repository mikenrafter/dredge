use clap::Parser;
use colored::Colorize;
use dredge::cli::{Cli, Command};
use dredge::format;
use dredge::input;
use dredge::query;
use dredge::analysis;
use dredge::output;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Formats) => {
            print_formats();
            return;
        }
        _ => {}
    }

    let max_bytes = input::resolve_max_bytes(cli.max_input_bytes);
    let input = match read_input(&cli.files, max_bytes) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    };
    if input.trim().is_empty() {
        eprintln!("{} No input. Provide log files or pipe data via stdin.", "error:".red().bold());
        eprintln!("\n  {} dredge <file.log>", "Usage:".dimmed());
        eprintln!("         cat logs/*.log | dredge summary");
        std::process::exit(1);
    }

    // Parse
    let (format, records) = format::parse_input(&input);

    if records.is_empty() {
        eprintln!("{} No records could be parsed from input.", "error:".red().bold());
        std::process::exit(1);
    }

    // Build filters
    let filters = match build_filters(&cli) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    };

    // Apply filters
    let filtered: Vec<_> = if filters.is_empty() {
        records
    } else {
        let combined = query::Filter::And(filters);
        records.into_iter().filter(|r| combined.matches(r)).collect()
    };

    // Determine mode
    let is_summary = matches!(cli.command, Some(Command::Summary))
        || (cli.filter.is_none()
            && cli.search.is_none()
            && cli.since.is_none()
            && cli.until.is_none()
            && cli.level.is_none()
            && cli.count_by.is_none()
            && cli.limit.is_none());

    // Count-by mode
    if let Some(ref field) = cli.count_by {
        let counts = query::count_by_field(&filtered, field);
        if cli.json {
            output::print_json_aggregation(&counts, field);
        } else {
            output::print_aggregation(&counts, field);
        }
        return;
    }

    // Summary mode
    if is_summary {
        let summary = analysis::generate_summary(&filtered, format);
        if cli.json {
            output::print_json_summary(&summary);
        } else {
            output::print_summary(&summary);
        }
        return;
    }

    // Query mode — show filtered records
    let display: Vec<_> = match cli.limit {
        Some(n) => filtered.into_iter().take(n).collect(),
        None => filtered,
    };

    if cli.json {
        output::print_json_records(&display);
    } else {
        output::print_records(&display, cli.verbose);
    }
}

fn read_input(files: &[PathBuf], max_bytes: usize) -> io::Result<String> {
    if files.is_empty() {
        if io::stdin().is_terminal() {
            return Ok(String::new());
        }
        return input::read_bounded(io::stdin().lock(), max_bytes);
    }

    let mut combined = String::new();
    for path in files {
        match input::read_file_bounded(path, max_bytes) {
            Ok(content) => {
                if combined.len() + content.len() > max_bytes {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        input::LIMIT_ERROR,
                    ));
                }
                combined.push_str(&content);
                if !content.ends_with('\n') {
                    combined.push('\n');
                }
            }
            Err(e) if e.kind() == io::ErrorKind::InvalidData => return Err(e),
            Err(e) => {
                eprintln!("{} {}: {}", "warning:".yellow().bold(), path.display(), e);
            }
        }
    }
    Ok(combined)
}

fn build_filters(cli: &Cli) -> Result<Vec<query::Filter>, String> {
    let mut filters = Vec::new();

    if let Some(ref expr) = cli.filter {
        filters.push(query::parse_filter(expr)?);
    }

    if let Some(ref text) = cli.search {
        filters.push(query::Filter::TextSearch(text.clone()));
    }

    if let Some(ref since_str) = cli.since {
        let dur = query::parse_duration(since_str)?;
        let cutoff = chrono::Utc::now() - dur;
        filters.push(query::Filter::TimeAfter(cutoff));
    }

    if let Some(ref until_str) = cli.until {
        let dur = query::parse_duration(until_str)?;
        let cutoff = chrono::Utc::now() - dur;
        filters.push(query::Filter::TimeBefore(cutoff));
    }

    if let Some(ref level_str) = cli.level {
        let level = dredge::Level::from_str_loose(level_str)
            .ok_or_else(|| format!("unknown level: '{}'", level_str))?;
        filters.push(query::Filter::LevelAtLeast(level));
    }

    Ok(filters)
}

fn print_formats() {
    println!("{}", "Supported log formats:".bold());
    println!();
    println!("  {}  One JSON object per line", "JSON Lines".cyan().bold());
    println!("           Fields: timestamp, level, message (+ any others)");
    println!();
    println!("  {}       key=value pairs, optionally quoted", "logfmt".cyan().bold());
    println!("           Fields: ts, level, msg (+ any others)");
    println!();
    println!("  {}          Apache/nginx access logs", "CLF".cyan().bold());
    println!("           192.168.1.1 - user [timestamp] \"GET /path HTTP/1.1\" 200 1234");
    println!();
    println!("  {}     CLF + referer + user-agent", "Combined".cyan().bold());
    println!();
    println!("  {}       RFC 3164 / RFC 5424", "syslog".cyan().bold());
    println!("           <priority>Mon DD HH:MM:SS host process[pid]: message");
    println!();
    println!("  {}   Unstructured text (fallback)", "plain text".cyan().bold());
    println!("           Extracts timestamps and log levels when possible");
    println!();
    println!("  {}", "Format is auto-detected from the first 10 lines.".dimmed());
}
