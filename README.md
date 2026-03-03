# dredge

A fast, smart log analysis tool for the terminal. Point it at your logs and it tells you what's wrong.

Dredge auto-detects log formats, clusters similar errors together, spots time trends, and gives you a clear summary instead of making you wade through walls of text.

## Install

```bash
cargo install --path .
```

## Quick start

```bash
# Analyze a log file — shows summary by default
dredge app.log

# Pipe from stdin
cat /var/log/*.log | dredge

# Filter to errors only
dredge --where 'level == "error"' app.log

# Search for text across all fields
dredge --search "timeout" app.log

# Count occurrences by field
dredge --count-by service app.log

# Filter by minimum level with a limit
dredge --level warn --limit 20 app.log
```

## What it does

### Auto-detect formats

Dredge reads the first 10 lines and figures out your log format. No configuration needed.

| Format | Example |
|--------|---------|
| **JSON Lines** | `{"timestamp":"...","level":"error","message":"..."}` |
| **logfmt** | `ts=... level=error msg="..."` |
| **Common Log Format** | `192.168.1.1 - - [timestamp] "GET /path" 200 1234` |
| **Combined Log Format** | CLF + referer + user-agent |
| **syslog** | `<34>Jan 5 14:32:01 host sshd[1234]: ...` |
| **Plain text** | Anything else — extracts levels and timestamps when possible |

### Summary analysis

When you run `dredge app.log` with no flags, you get a summary:

```
╭─ Summary ────────────────────────────────────────╮
│ 25,000 records | 2h 15m span | 3.1 rec/sec       │
│ Format: JSON Lines                                │
╰───────────────────────────────────────────────────╯

  Levels
    INFO    23,422  ████████████████████  93.7%
    WARN       891  ████░░░░░░░░░░░░░░░░   3.6%
    ERROR      342  █░░░░░░░░░░░░░░░░░░░   1.4%
    DEBUG      345  █░░░░░░░░░░░░░░░░░░░   1.4%

  Top Errors
    1. Connection timeout to <IP>:<N> (201× | first 2h ago | last 5m ago)
    2. Auth token expired for user <UUID> (47× | first 45m ago | last 2m ago)
    3. File not found: <PATH> (31× | first 1d ago | last 10m ago)

  Trends
    ⚠ Error rate increasing 2.3× over recent window
    ⚡ Spike at 14:30 UTC (45 errors vs avg 8)

  Top Fields
    service: auth (45%), api (30%), worker (25%)
    host: prod-1 (40%), prod-2 (35%), prod-3 (25%)
```

Error clustering normalizes variable parts (IPs, UUIDs, numbers, paths, URLs) so structurally identical errors group together. Trend detection identifies increasing error rates, spikes, and quiet periods.

### Filtering and querying

```bash
# Field equality
dredge --where 'level == "error"' app.log
dredge --where 'service == "auth"' app.log

# Numeric comparison
dredge --where 'status >= 500' access.log
dredge --where 'duration_ms > 1000' app.log

# Substring match
dredge --where 'service contains "auth"' app.log

# Text search across all fields
dredge --search "connection refused" app.log

# Minimum log level
dredge --level warn app.log

# Combine filters
dredge --where 'level == "error"' --search "timeout" app.log

# Time-based (relative to now)
dredge --since 1h app.log
dredge --since 30m --until 5m app.log
```

### Aggregation

```bash
# Count by any field
dredge --count-by service app.log
dredge --count-by host app.log
dredge --count-by status access.log
```

### Output formats

```bash
# Default: colored terminal output
dredge --where 'level == "error"' app.log

# Verbose: show all fields on separate lines
dredge -v --where 'level == "error"' app.log

# JSON output for piping
dredge --json --where 'level == "error"' app.log
dredge --json summary app.log
```

## Duration syntax

For `--since` and `--until`:

| Unit | Example |
|------|---------|
| `s` | `30s` |
| `m` | `5m` |
| `h` | `1h` |
| `d` | `7d` |
| `w` | `1w` |
| compound | `2h30m` |

## Design

Dredge is built around four modules:

- **format** — Auto-detection and parsing across 6 log formats, with heuristic field mapping and flexible timestamp parsing (ISO 8601, RFC 2822, CLF, syslog, Unix epoch)
- **query** — Filter expressions with field access, comparison operators, text/regex search, time ranges, and composable `And`/`Or`/`Not` logic
- **analysis** — Error clustering via message normalization (replacing UUIDs, IPs, paths, etc. with placeholders), time-bucketed trend detection, and summary generation
- **output** — Colored terminal display with box-drawing, bar charts, and relative timestamps; JSON output mode for scripting

## License

MIT
