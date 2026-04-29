# Library Migration Plan

Move reusable Git behavior from `grit/src` into `grit-lib/src` while keeping the `grit` binary focused on argv parsing, process setup, terminal/editor interaction, stdout/stderr formatting, and exit-code mapping.

## Leaf Utilities

- [x] Move Git path helpers from `grit/src/git_path.rs` to `grit-lib/src/git_path.rs`.
- [x] Replace CLI callers of `crate::git_path` with `grit_lib::git_path`.
- [x] Move URL rewrite helpers from `grit/src/url_rewrite.rs` to `grit-lib/src/url_rewrite.rs`.
- [x] Replace CLI callers of `crate::url_rewrite` with `grit_lib::url_rewrite`.
- [x] Move `.git*` HFS/NTFS safety checks from `grit/src/dotfile.rs` to `grit-lib/src/dotfile.rs`.
- [x] Replace CLI callers of `crate::dotfile` with `grit_lib::dotfile`.
- [x] Move local transport path guard helpers from `grit/src/transport_path.rs` to `grit-lib/src/transport_path.rs`.
- [x] Replace any CLI callers of transport path helpers with `grit_lib::transport_path`.
- [x] Move Git binary base85 codec from `grit/src/git_binary_base85.rs` to `grit-lib/src/git_binary_base85.rs`.
- [x] Replace CLI callers of `crate::git_binary_base85` with `grit_lib::git_binary_base85`.
- [x] Convert moved leaf helpers away from `anyhow` and unit errors where they become public library APIs.
- [x] Run `cargo fmt` and `cargo check -p grit-rs` after each leaf batch.

## Wire And Protocol Framing

- [x] Split CLI command helpers out of `grit/src/pkt_line.rs` from pure pkt-line encode/decode functions.
- [x] Move pkt-line constants, `Packet`, read helpers, write helpers, and sideband encode/decode routines to `grit-lib/src/pkt_line.rs`.
- [x] Keep `cmd_pack`, `cmd_unpack`, and other stdin/stdout test-tool shims in the CLI, delegating to `grit_lib::pkt_line`.
- [x] Replace transport and upload-pack callers of `crate::pkt_line` with `grit_lib::pkt_line`.
- [x] Move pure protocol version parsing and `GIT_PROTOCOL` merge helpers from `protocol_wire` into a library module that accepts explicit config/env inputs.
- [x] Keep process spawning and `Command` mutation at the CLI boundary unless the library API accepts an explicit process abstraction.

## Pathspec Dedupe

- [x] Compare CLI `pathspec.rs` behavior against `grit-lib/src/pathspec.rs` for `simple_length`, glob detection, magic parsing, and matching.
- [x] Move `--pathspec-from-file` parsing and C-style unquoting into `grit-lib::pathspec`.
- [x] Replace CLI callers of `parse_pathspecs_from_source` with the library function.
- [x] Replace CLI pathspec matching helpers with `grit_lib::pathspec` where semantics already match.
- [x] Replace simple glob-detection callers with `grit_lib::pathspec::has_glob_chars`.
- [x] Delete duplicate CLI pathspec helpers once all callers use the library.
- [x] Run focused tests that cover pathspec matching paths (`./scripts/run-tests.sh t6130-pathspec-noglob.sh`, 21/21).

## Display And Formatting

- [x] Move `git_column` layout types and pure formatting routines to `grit-lib`.
- [x] Parameterize TTY-dependent column auto mode so the library does not read terminal state implicitly.
- [x] Keep `git column` stdin/stdout wiring in the CLI.
- [x] Move branch tracking computations from `branch_tracking.rs` into a library module with typed errors.
- [x] Keep user-facing advice text and exact stdout/stderr composition in the CLI unless shared by multiple commands.
- [x] Move `branch_ref_format` atom expansion to the library after deciding its error type and module placement.

## Commit And Identity

- [x] Move extra commit encoding helpers from `grit/src/git_commit_encoding.rs` into `grit-lib/src/commit_encoding.rs` or a sibling module.
- [x] Replace CLI re-export wrapper uses with direct `grit_lib::commit_encoding` calls.
- [x] Split identity resolution from `grit/src/ident.rs` into an env-injected library API.
- [x] Keep direct `std::env` reads in CLI wrappers until an explicit environment provider exists.
- [x] Preserve exact identity error messages and exit-code mapping in the binary.

## Receive And Transport

- [x] Split pure receive-pack config helpers from `receive_ingest.rs` into `grit-lib`.
- [x] Move `receive_unpack_limit`, `max_input_size_from_config`, and `pack_object_count` into a library receive module.
- [x] Keep subprocess-based pack ingestion orchestration in the CLI until pack ingestion can call library APIs directly.
- [x] Move protocol allow/deny policy into `grit-lib` with explicit config/env parameters.
- [x] Keep HTTP/SSH process, tracing, and network orchestration in the CLI until a dedicated library transport boundary is designed.

## Verification

- [x] After each migration batch, run `cargo fmt`.
- [x] After each migration batch, run `cargo check -p grit-rs`.
- [x] For behavior-sensitive moves, run the smallest relevant upstream harness file through `./scripts/run-tests.sh`.
- [x] Use `ReadLints` on edited files and fix newly introduced diagnostics.
