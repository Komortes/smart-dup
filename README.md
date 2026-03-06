# Smart Duplicate Finder (Rust)

`smart-dup` is a CLI tool for finding duplicate files by real content and removing them safely.

It is built for real directories with large file counts and focuses on fast duplicate detection with a safe, explicit deletion workflow.

## What it does

- Scans one or many folders recursively
- Detects exact duplicates by content hash (`blake3`)
- Uses size pre-grouping to reduce hashing workload
- Hashes files in parallel with optional thread limit (`--threads`)
- Shows progress while scanning and hashing
- Exports duplicate groups to JSON and CSV
- Supports safe delete workflow from JSON report
- Requires explicit safe mode (`--dry-run`, `--interactive`, or `--yes`)
- Supports keep strategies: `oldest`, `newest`, `lexicographic`, `path-priority`
- Supports Trash-based deletion on macOS/Linux/Windows with fallback to direct delete
- Verifies file hash before deleting (can be disabled with `--no-verify-hash`)
- Supports photo mode:
  - exact duplicates by content (`photos`)
  - similar duplicates via perceptual dHash (`photos --similar`)

## Build & Run

```bash
cargo build
cargo run -- --help
```

Run tests:

```bash
cargo test
```

## Install

From source:

```bash
cargo install --path .
```

From GitHub releases:

macOS/Linux:

```bash
./scripts/install.sh v0.2.0
```

Windows (PowerShell):

```powershell
.\scripts\install.ps1 -Version v0.2.0
```

Notes:

- Override repo if needed via `SMARTDUP_REPO=<owner/repo>` for `install.sh`.
- Windows installer supports x64 release assets.

## Releases

Automated releases are configured in GitHub Actions:

- Workflow: `.github/workflows/release.yml`
- Preflight workflow: `.github/workflows/release-verify.yml` (build/package check before tagging)
- Trigger: push tag `v*` (example: `v0.2.0`)
- Artifacts:
  - `x86_64-unknown-linux-gnu`
  - `x86_64-unknown-linux-musl`
  - `x86_64-pc-windows-msvc`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
- Each release also includes `SHA256SUMS.txt`

Publish flow:

```bash
# 1) bump version in Cargo.toml
# 2) commit changes
git tag v0.2.0
git push origin main --tags
```

## CLI

```bash
smart-dup <COMMAND>
```

Commands:

- `scan` scan folders and build duplicate groups
- `delete` remove duplicates from JSON report
- `photos` scan photo files for exact or similar duplicates

## Scan Examples

Basic scan:

```bash
cargo run -- scan ~/Downloads
```

Scan multiple roots, skip small files, set hashing workers:

```bash
cargo run -- scan ~/Downloads ~/Desktop --min-size 1MB --threads 4
```

CI/script friendly output (no progress bars, summary only):

```bash
cargo run -- scan ~/data --no-progress --quiet
```

Custom ignore rules:

```bash
cargo run -- scan ~/data --ignore cache --ignore vendor
```

Disable built-in ignores:

```bash
cargo run -- scan ~/data --no-default-ignores
```

Export report files:

```bash
cargo run -- scan ~/data --json out.json --csv out.csv
```

Notes:

- Built-in ignores: `.git`, `node_modules`, `target`
- `--min-size` supports values like `4096`, `512KB`, `1MB`, `2GB`

## Safe Delete Examples

Preview only (no file changes):

```bash
cargo run -- delete --from out.json --dry-run --keep oldest
```

Script-friendly summary output:

```bash
cargo run -- delete --from out.json --dry-run --quiet
```

Interactive delete:

```bash
cargo run -- delete --from out.json --interactive --keep newest
```

Non-interactive confirmed delete:

```bash
cargo run -- delete --from out.json --yes --keep newest
```

Path-priority keep rule:

```bash
cargo run -- delete --from out.json --dry-run --keep path-priority \
  --prefer-path ~/Photos --prefer-path /Volumes/Archive
```

Force Trash mode:

```bash
cargo run -- delete --from out.json --interactive --trash
```

Force direct delete instead of Trash:

```bash
cargo run -- delete --from out.json --interactive --no-trash
```

Skip hash verification (faster, less safe):

```bash
cargo run -- delete --from out.json --interactive --no-verify-hash
```

Hard safety cap on real deletions:

```bash
cargo run -- delete --from out.json --yes --max-delete 100
```

Hard safety cap by total bytes:

```bash
cargo run -- delete --from out.json --yes --max-delete-bytes 500MB
```

Reject stale reports older than a threshold:

```bash
cargo run -- delete --from out.json --yes --max-report-age-secs 3600
```

Fail fast in automation if any delete fails or hash mismatch is detected:

```bash
cargo run -- delete --from out.json --yes --strict --quiet
```

Safety rule:

- If you do **not** pass `--dry-run`, you must pass `--interactive` or `--yes`.
- `--interactive` and `--yes` are mutually exclusive.

## Photos Examples

Exact duplicate photos (content hash):

```bash
cargo run -- photos ~/Pictures
```

Similar photos (perceptual dHash):

```bash
cargo run -- photos ~/Pictures --similar --threshold 8
```

Notes:

- Similar mode backend per OS:
  - macOS: `sips`
  - Linux: `magick`/`convert` (ImageMagick)
  - Windows: PowerShell + `System.Drawing`
- Exact photo mode works across all supported OS targets.
- For Linux similar mode, install ImageMagick package (`magick` or `convert` command).

## Exit Codes

- `0` success
- `2` CLI usage/argument errors (reported by `clap`)
- `3` input/report errors (for example missing or invalid `--from` JSON)
- `4` safety policy violations (for example missing safe mode, stale report, delete limits)
- `5` strict mode failure (`--strict` + failed deletions or hash mismatches)
- `6` runtime errors (for example export write failures)

## Output Model

JSON export contains:

- roots
- scan timestamp
- summary counters
- duplicate groups (`file_size`, `content_hash`, files list)

CSV export is flat per-file rows:

- `group_index`
- `content_hash`
- `file_size`
- `path`
- `modified_unix_secs`

## Cross-Platform Checklist

- CI tests run on Linux/macOS/Windows (`.github/workflows/ci.yml`)
- Keep filesystem logic in `std::path` (avoid OS-specific path parsing)
- Use `--trash` for recycle-bin deletion on all supported desktop OSes, with fallback to direct delete
- Linux release builds include `x86_64-unknown-linux-musl` for better portability across distros
- Before each release, run a real smoke test on:
  - macOS (Intel + Apple Silicon)
  - Windows 11
  - Linux (Ubuntu/Debian family at minimum)
