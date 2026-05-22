use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "dredge",
    about = "A fast, smart log analysis tool for the terminal",
    long_about = "Point dredge at your logs and it tells you what's wrong.\n\
        Auto-detects formats, clusters similar errors, spots trends,\n\
        and gives you a clear summary instead of walls of text.\n\n\
        TIPS:\n\
        • Pipe nginx or app logs directly — dredge reads stdin when it is not a TTY.\n\
        • Use summary mode (default) for error clusters; add --where to filter.\n\
        • JSON Lines and logfmt are detected from the first lines of input.",
    after_help = "EXAMPLES:\n  \
        dredge summary /var/log/app.log\n  \
        cat access.log | dredge --where 'status >= 500'\n  \
        dredge --count-by level --json errors.jsonl\n  \
        dredge formats\n\n\
        ENVIRONMENT:\n  \
        DREDGE_MAX_INPUT_BYTES  Default input cap when --max-input-bytes is omitted."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(global = true)]
    pub files: Vec<PathBuf>,

    #[arg(long = "max-input-bytes", global = true)]
    pub max_input_bytes: Option<usize>,

    #[arg(short = 'w', long = "where", global = true)]
    pub filter: Option<String>,

    #[arg(short = 's', long = "search", global = true)]
    pub search: Option<String>,

    #[arg(long = "since", global = true)]
    pub since: Option<String>,

    #[arg(long = "until", global = true)]
    pub until: Option<String>,

    #[arg(short = 'l', long = "level", global = true)]
    pub level: Option<String>,

    #[arg(short = 'c', long = "count-by", global = true)]
    pub count_by: Option<String>,

    #[arg(short = 'n', long = "limit", global = true)]
    pub limit: Option<usize>,

    #[arg(long = "json", global = true)]
    pub json: bool,

    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    Summary,
    Formats,
}
