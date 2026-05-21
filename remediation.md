# Security Remediation Report: dredge

## Audit Metadata
- Repository: https://github.com/JoaquinCampo/dredge
- Audited commit: `c5b6240cfdec6f57ebd049968c5288510fd8d6d6`
- Audit date: 2026-04-24
- Auditor workflow: staged source review + dependency advisory scan

## Scope and Method
Three-pass staged review was used:
1. Stage 1: docs removed, full-line comments removed.
2. Stage 2: non-doc files restored (comments restored), docs removed.
3. Stage 3: full docs restored.

Review focus:
- odd network behavior and suspicious outbound requests
- odd encodings / parser hazards
- test code boundary and production contamination
- file operations and input trust boundaries
- misleading comments and suspicious links
- package CVE/advisory exposure and mitigation options

## Executive Summary
No hidden network behavior or command-execution primitives were found in runtime logic. Main practical security risk is local resource exhaustion due to reading all stdin/files into memory. Dependency advisory scan found no active vulnerabilities, but found `atty` informational warnings (unmaintained; unsound advisory on Windows).

## Findings by Severity

### Medium: Potential Local DoS via Full Input Buffering
- Risk: large log inputs can exhaust memory / degrade availability.

Evidence:
- Full stdin read: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/main.rs#L170
- Full file read loop: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/main.rs#L175

Remediation:
1. Stream input incrementally (line/chunk processing) rather than full read.
2. Add max-input-size guard with safe default and override flag.
3. Add backpressure-friendly aggregation strategy for huge files.

### Low: Regex-heavy parser surface (but Rust regex engine is linear-time)
- Risk: parser complexity and false-positive/false-negative behavior under weird logs.
- Note: Rust `regex` avoids catastrophic backtracking patterns by design, reducing ReDoS class risk.

Evidence:
- Format regex compilation: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/format/mod.rs#L13
- Query parser regex usage: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/query/mod.rs#L159
- JSON parse checks from untrusted lines: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/format/mod.rs#L106

Remediation:
1. Keep parser behavior deterministic under malformed lines.
2. Add corpus tests for adversarially large and malformed records.
3. Add parse budget metrics in benchmark CI.

### Low: Direct File Read Surface
- Risk: user-specified path reads are expected for CLI tools but should be bounded and observable.

Evidence:
- File reads in user-supplied path list: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/main.rs#L175

Remediation:
1. Provide optional allowlist root flag for enterprise use.
2. Emit warning on extremely large files before reading all content.

## Stage-by-Stage Results

### Stage 1 (comments removed, docs removed)
- No odd outbound network requests in runtime code.
- No hidden encoding channels beyond expected log parsing.
- No command execution API usage in runtime flow.
- Test boundary appeared correct.

### Stage 2 (comments restored, docs removed)
- Comments were mostly descriptive and aligned with implementation.
- No suspicious links or covert behavior instructions found in code comments.

### Stage 3 (docs restored)
- No malicious or manipulative links found in README.
- Documentation appears consistent with observed behavior.

Reference:
- README quick start: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/README.md#L12

## Test-to-Production Boundary
No evidence that test code is compiled into production binaries.

Evidence:
- Query tests gated: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/query/mod.rs#L312
- Format tests gated: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/format/mod.rs#L603
- Analysis tests gated: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/analysis/mod.rs#L354
- Output tests gated: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/src/output/mod.rs#L538

## Dependency CVE/Advisory Review
Scanner: `cargo-audit` via Nix shell (`nix shell nixpkgs#cargo-audit -c cargo-audit audit --file Cargo.lock`)

Result summary:
- Vulnerabilities found: none
- Informational advisories:
  - `RUSTSEC-2024-0375` (`atty` unmaintained): https://rustsec.org/advisories/RUSTSEC-2024-0375.html
  - `RUSTSEC-2021-0145` (`atty` potential unaligned read on Windows): https://rustsec.org/advisories/RUSTSEC-2021-0145.html

Dependency location:
- Direct `atty` dependency: https://github.com/JoaquinCampo/dredge/blob/c5b6240cfdec6f57ebd049968c5288510fd8d6d6/Cargo.toml#L15

Attack surface interpretation:
- Linux-only operation has lower practical exposure for the unsound advisory.
- Cross-platform distribution, especially Windows targets, should treat this as higher urgency.

Remediation:
1. Remove `atty` dependency and migrate to standard terminal detection APIs.
2. Re-run lockfile update and cargo-audit after migration.
3. Add CI advisory gating policy.

## Three Security Breakdowns

### 1. Exploitability-first view
- No obvious RCE primitive found. Most realistic abuse path is very large input causing memory pressure.

### 2. Supply-chain view
- No active vulnerabilities, but `atty` advisories indicate maintenance and platform-specific risk debt.

### 3. Operational resilience view
- Robustness under extreme log volumes is the most important hardening area.

## Prioritized Remediation Plan
1. Replace `atty` with standard terminal detection and remove advisory debt.
2. Stream processing and input caps for stdin and file inputs.
3. Add adversarial log corpus tests and parser budget checks.
4. Add CI `cargo-audit` enforcement.

## Verification Checklist
- [ ] `atty` removed from dependency tree.
- [ ] Large inputs no longer require full-buffer reads.
- [ ] Parser behavior remains stable on malformed/adversarial logs.
- [ ] `cargo-audit` passes in CI for the lockfile.
