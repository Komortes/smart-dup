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
- Requires explicit safe mode (`--dry-run` or `--interactive`)
- Supports keep strategies: `oldest`, `newest`, `lexicographic`, `path-priority`
- Supports macOS Trash-based deletion with fallback to direct delete
- Verifies file hash before deleting (can be disabled with `--no-verify-hash`)

## Build & Run

```bash
cargo build
cargo run -- --help
```

Run tests:

```bash
cargo test
```

## CLI

```bash
smart-dup <COMMAND>
```

Commands:

- `scan` scan folders and build duplicate groups
- `delete` remove duplicates from JSON report
- `photos` reserved command for future photo mode

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

Fail fast in automation if any delete fails or hash mismatch is detected:

```bash
cargo run -- delete --from out.json --yes --strict --quiet
```

Safety rule:

- If you do **not** pass `--dry-run`, you must pass `--interactive`.

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
