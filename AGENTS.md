# AGENTS.md — N-coding

## Environment

- **Nix dev shell**: `nix develop` (or `direnv allow`). The shell hook **drops you into nushell** (`nu`), not bash.
- **Without Nix**: you need Rust stable + openssl + ncurses + `nushell`, `ripgrep`, `jujutsu` on PATH.
- **API key**: `DEEPSEEK_API_KEY` env var required at runtime.
- **CI shell** (non-interactive): `nix develop .#ci` — skips the nushell entry and dev tools.

## Build / Test / Lint

```sh
cargo build              # or `cargo b`
cargo test               # `cargo nextest run` preferred (installed in dev shell)
cargo clippy -- -D warnings
cargo fmt -- --check
```

- `RUSTFLAGS="-C target-cpu=native"` is set in the Nix shell. Keep it for local builds.
- `RUST_BACKTRACE=1` is set in the Nix shell.

## Architecture

This is a **TUI coding agent** (ratatui + crossterm). It is NOT a library — `src/main.rs` is the single entrypoint.

### Module ownership (see `src/*/README.md` for full context)

| Module | Job |
|--------|-----|
| `command/` | Text-command parser + 5 built-in executors (Shell, FilesOperator, ToolCall, SubAgentTask, AgentSkills) |
| `session/` | `.ncoding/sessions/*.json` persistence, `/undo` file backups |
| `api/` | DeepSeek SSE streaming + reasoning_content handling |
| `config/` | KDL config loader (3-tier: env → `n_coding.kdl` → `~/.config/ncoding/config.kdl`) |
| `tui/` | ratatui app loop, rendering, input, `/model` panel, Everforest theme |
| `prompt/` | System prompt template assembly |

### Key design decisions (not obvious from filenames)

1. **No OpenAI tool_call / function_call.** Commands are parsed from model text output via regex (`<<<[Command]>>>...<<<[__END__]>>>`). Adding a native tool_call would violate the design.
2. **Shell commands use nushell (`/usr/bin/nu -c`), NEVER bash.** The shell executor hardcodes `nu`.
3. **reasoning_content must be round-tripped.** DeepSeek does not auto-include it; `api::client` stores it and sends it back in the next request's assistant message.
4. **FilesOperator edit uses exact-string-replace** (like OpenCode/Claude Code). Old string must be unique in the file; model must `read` before `edit`.
5. **Config is KDL format**, parsed via `knus` crate. Not TOML, not JSON.
6. **Session naming is auto-generated** via a separate lightweight API call (using `sub_model`, default `deepseek-v4-flash`).

## Development phases

See `phases.md` at root — it maps each `src/` directory to the 4 phases. Currently the project is a **skeleton** (Phase 1 work starting).

## Conventions

- **No comments** in Rust source unless essential (following the project's minimal-comment style already in place).
- **README.md files** in each `src/` subdirectory are the design-level docs — keep them in sync with significant architecture changes.
- **Everforest dark theme** is the only theme. Color constants live in `src/tui/theme.rs`.
- **Error handling**: use `anyhow::Result` for application-level, `std::io::Error` for terminal I/O.
- **TUI testing**: the app runs in an alternate screen — manual testing only. Print-debug with `tracing` (logs to stderr or `.ncoding/n-coding.log`).
