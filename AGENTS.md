# AGENTS.md — N-coding

## Environment

- **Nix dev shell**: `nix develop` (or `direnv allow`).
- **Without Nix**: you need Rust stable + openssl + ncurses + `bash`, `ripgrep`, `jujutsu` on PATH.
- **API key**: `DEEPSEEK_API_KEY` env var required at runtime.
- **CI shell** (non-interactive): `nix develop .#ci`.
- **Logging**: `RUST_LOG` env controls level (default `info`). Logs go to `.ncoding/n-coding.log` via `tracing`.

## Build / Test / Lint

```sh
cargo build              # or `cargo b`
cargo test               # `cargo nextest run` preferred (installed in dev shell)
cargo clippy -- -D warnings
cargo fmt -- --check
```

- `RUST_BACKTRACE=1` is set in the Nix shell.

## Architecture

This is a **TUI coding agent** (ratatui + crossterm). Single entrypoint: `src/main.rs`.

### Module ownership

| Module | Job |
|--------|-----|
| `command/` | Text-command parser + 7 executors (Shell, FilesOperator, ToolCall, SubAgentTask, AgentSkills, CheckList, AgentLogs) |
| `session/` | `.ncoding/sessions/*.json` persistence, lazy activation, `/undo` |
| `api/` | DeepSeek SSE streaming + reasoning_content handling, session name generation |
| `config/` | KDL config loader (3-tier: env → `n_coding.kdl` → `~/.config/ncoding/config.kdl`) |
| `tui/` | ratatui app loop, rendering, input, Everforest theme |
| `prompt/` | System prompt template assembly |

### Key design decisions (not obvious from filenames)

1. **No OpenAI tool_call/function_call.** Commands parsed from `《[Command]|character|》` ... `《[End]|Command|》` with `【key】value【key】` pairs.
2. **Shell uses bash (`/usr/bin/bash -c`), NOT nushell.** Changed from nu to improve model familiarity. Recommend rg (ripgrep) for file search.
3. **reasoning_content must be round-tripped.** DeepSeek does not auto-include it.
4. **FilesOperator edit uses exact-string-replace.** Model must `read` before `edit`.
5. **Config is KDL format**, parsed via `kdl` crate. Not TOML/JSON.
6. **`---` in KV content is NOT a block separator.** Prevent markdown horizontal rules from mis-parsing.
7. **Non-blocking command execution.** results arrive via `TuiEvent::CommandsCompleted`.
8. **Auto-continue is unlimited.** No cap on consecutive model-driven turns.
9. **System injection format: `【|SYSTEM|】\n...\n【|SYSTEM|】`** — replaces old `<(<(SYSTEM...)>)>`.
10. **No automatic context truncation** — `max_context_messages` removed to preserve cache hit rate. Only `/clear` truncates (user-initiated).
11. **Session lazy creation** — file created only on first real user input, not at startup.
12. **Slash commands**: `/init`, `/compact`, `/clear` (display only), `/session list|switch <name or id>|rename|delete`, `/undo`.

## Development conventions

- **No comments in Rust source** unless absolutely essential (project style).
- **README.md in each `src/` subdirectory** is the design doc — keep them in sync.
- **Error handling**: `anyhow::Result` for application-level, `std::io::Error` for terminal I/O.
- **`#![allow(dead_code)]`** on several modules (input.rs, render.rs, model_panel.rs, backup.rs) — these are Phase 4 placeholders.
- **TUI testing**: the app runs in an alternate screen — manual testing only. Use `tracing` for debug logging.
- **Everforest dark** is the only theme. Colors in `src/tui/theme.rs`.

## Common shell commands (this workspace)

```sh
# Finding files (use rg, NOT find)
rg "pattern" src/**/*.rs
rg -l "struct Foo" src/

# Directory listing
ls src/**/*.rs | sort

# Running tests
cargo test                              # all tests
cargo test parser                       # single module
cargo test -- --nocapture               # with output

# Checking just compilation (fast)
cargo check

# Fixing auto-fixable warnings
cargo fix --bin "n-coding" --allow-dirty
```

## Phase status

Phase 1-3: ✅ Complete
Phase 4: 🚧 In progress (model panel, markdown, resize, shell env injection)
