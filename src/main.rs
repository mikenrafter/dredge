use clap::{Parser, Subcommand};
use colored::Colorize;
use dredge::format;
use dredge::query;
use dredge::analysis;
use dredge::output;
use std::io::{self, Read};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "dredge",
    about = "A fast, smart log analysis tool for the terminal",
    long_about = "Point dredge at your logs and it tells you what's wrong.\n\
        Auto-detects formats, clusters similar errors, spots trends,\n\
        and gives you a clear summary instead of walls of text.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Log files to analyze (reads stdin if none provided)
    #[arg(global = true)]
    files: Vec<PathBuf>,

    /// Filter expression (e.g., 'level == "error"', 'status >= 500')
    #[arg(short = 'w', long = "where", global = true)]
    filter: Option<String>,

    /// Text search across all fields
    #[arg(short = 's', long = "search", global = true)]
    search: Option<String>,

    /// Show records since duration ago (e.g., 1h, 30m, 7d)
    #[arg(long = "since", global = true)]
    since: Option<String>,

    /// Show records until duration ago (e.g., 1h, 30m)
    #[arg(long = "until", global = true)]
    until: Option<String>,

    /// Minimum log level to show (trace, debug, info, warn, error, fatal)
    #[arg(short = 'l', long = "level", global = true)]
    level: Option<String>,

    /// Count records grouped by field
    #[arg(short = 'c', long = "count-by", global = true)]
    count_by: Option<String>,

    /// Maximum number of records to display
    #[arg(short = 'n', long = "limit", global = true)]
    limit: Option<usize>,

    /// Output as JSON
    #[arg(long = "json", global = true)]
    json: bool,

    /// Show verbose output with all fields
    #[arg(short = 'v', long = "verbose", global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze logs and show a summary report (default if no flags given)
    Summary,
    /// List supported log formats
    Formats,
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Formats) => {
            print_formats();
            return;
        }
        _ => {}
    }

    // Read input
    let input = read_input(&cli.files);
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

fn read_input(files: &[PathBuf]) -> String {
    if files.is_empty() {
        // Check if stdin is a pipe
        if atty::is(atty::Stream::Stdin) {
            return String::new();
        }
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).unwrap_or_default();
        buf
    } else {
        let mut combined = String::new();
        for path in files {
            match fs::read_to_string(path) {
                Ok(content) => {
                    combined.push_str(&content);
                    if !content.ends_with('\n') {
                        combined.push('\n');
                    }
                }
                Err(e) => {
                    eprintln!("{} {}: {}", "warning:".yellow().bold(), path.display(), e);
                }
            }
        }
        combined
    }
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
