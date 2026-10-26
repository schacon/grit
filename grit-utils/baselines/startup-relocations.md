# PIE startup relocation investigation (#2028)

Base: `f0e7cddd530a27366b74a4901b778395e404b458` (origin/main at checkout). Both variants use that exact source. Candidate changes only the CLI final link with `-Wl,-z,pack-relative-relocs`; the map option is diagnostic. No codegen units, LTO, panic policy, PIE, ASLR, features, library algorithms or CLI behavior change. RELR is not enabled for shipping pending a loader-baseline decision.

## Environment and method

2026-10-26, Linux x86_64, 4 vCPU AMD EPYC 9V74, 16 GiB RAM, Rust 1.99.0, bundled LLD 23.1.1, glibc 2.39, system Git 2.55.0. This is not the historical Xeon host. Stock and candidate `release` and `dist` artifacts were frozen before measurement. Five warmups precede 60 recorded runs per tool per scenario, rotating tool order. Builds and tests were stopped during timing.

`startup_compare` uses the daily runner helper and a small C fork/exec/wait4 parent. The `native` blocks below are full process times and independent per-child peak RSS/page faults. The ordinary v1 `git`/`grit` fields use the existing daily helper (after spawn through exit); the docs tables generated from the site manifest use those fields. All individual wall samples and fault pairs are retained. `/usr/bin/time -v` supplies ten further independent RSS samples per tiny command/tool/profile; `LD_DEBUG=statistics` supplies 30 loader-only observations. These are in `startup-relocations-loader-rss.json`, separate from wall time.

Fixtures are generated through grit-lib and pass system `git fsck --strict --no-dangling`: tiny (1 file/1 commit/1 local branch), many-refs (100 files/100 commits/20k refs), wide (20k files/100 commits/100 refs), deep (100 files/20k commits/100 refs). Branch, status, resolve, help and a 1000-entry log run on every fixture; stock and candidate must have identical stdout/stderr and successful exits. Grit branch uses `-n 20`; Git branch lists all branches, so its many-ref output volume differs.

## Full process time and peak RSS

Times are median ± sample standard deviation, milliseconds. Peak RSS is the maximum of 60 independent C-parent readings, KiB.

| Profile / tiny command | Git ms | Stock ms | RELR ms | Git / stock / RELR peak KiB |
| --- | ---: | ---: | ---: | ---: |
| release / branch | 1.237 ± 0.040 | 2.177 ± 0.056 | 2.144 ± 0.063 | 4788 / 12272 / 11668 |
| release / status | 1.524 ± 0.063 | 2.699 ± 0.118 | 2.662 ± 0.102 | 5384 / 14248 / 13440 |
| release / resolve | 1.127 ± 0.048 | 2.292 ± 0.086 | 2.264 ± 0.073 | 4628 / 12236 / 11432 |
| release / help | 1.006 ± 0.043 | 3.102 ± 0.125 | 3.064 ± 0.135 | 4080 / 8648 / 7852 |
| dist / branch | 1.182 ± 0.061 | 2.013 ± 0.042 | 1.858 ± 0.075 | 4780 / 8896 / 9184 |
| dist / status | 1.428 ± 0.022 | 2.408 ± 0.036 | 2.243 ± 0.032 | 5380 / 10420 / 10720 |
| dist / resolve | 1.068 ± 0.018 | 2.083 ± 0.022 | 1.926 ± 0.027 | 4656 / 9008 / 9312 |
| dist / help | 0.962 ± 0.018 | 2.934 ± 0.100 | 2.790 ± 0.060 | 4104 / 8956 / 8668 |

| Profile / larger case | Git ms | Stock ms | RELR ms | Git / stock / RELR peak KiB |
| --- | ---: | ---: | ---: | ---: |
| release / many-refs branch | 43.284 ± 1.343 | 10.820 ± 0.482 | 10.902 ± 0.384 | 19744 / 17764 / 16880 |
| release / wide status | 15.624 ± 1.162 | 16.353 ± 0.802 | 16.351 ± 0.677 | 9364 / 17828 / 17092 |
| release / deep log | 6.158 ± 0.175 | 9.163 ± 0.190 | 9.162 ± 0.170 | 10100 / 19740 / 18964 |
| dist / many-refs branch | 43.398 ± 0.991 | 10.889 ± 0.352 | 10.718 ± 0.380 | 19628 / 14832 / 14812 |
| dist / wide status | 13.604 ± 0.998 | 14.300 ± 0.381 | 14.066 ± 0.242 | 9364 / 13968 / 14076 |
| dist / deep log | 6.031 ± 0.055 | 8.452 ± 0.093 | 8.272 ± 0.093 | 10028 / 16056 / 16212 |

Release tiny startup improves about 1–2%; dist improves about 5–8%. Larger cases remain close (release many-ref branch is 0.8% slower, within its sample spread). Release peak RSS falls roughly 0.6–0.8 MiB. Dist peak RSS does not consistently decrease: its observed maxima rise by up to about 0.3 MiB on tiny branch/status/resolve. Ten `/usr/bin/time -v` samples put the corresponding median differences at +34/+58/+48 KiB, while help falls by 242 KiB. These profile-dependent layout effects are a limit of the result; RELR is not a general RSS win.

## Relocations, layout and loader work

| Profile | Stock / RELR binary bytes | Stock / RELR relocation metadata bytes | Relative pointer slots (both) | Pages with relative fixups (both) |
| --- | ---: | ---: | ---: | ---: |
| release | 33,743,040 / 32,964,904 | 792,936 / 13,584 | 32,865 | 147 |
| dist | 24,411,872 / 23,957,320 | 464,328 / 9,872 | 19,174 | 91 |

Metadata includes `.rela.dyn` plus `.relr.dyn` when present; the unchanged tiny `.rela.plt` is excluded. RELR compresses metadata, not pointer slots: it still dirties the same number of pages containing relative fixups. `.text`, `.rodata`, and `.data.rel.ro` sizes are identical within each profile, but their addresses shift. ELF section details, binary hashes and attribution are in `startup-relocations-elf.json`.

The non-LTO linker map attributes 8,902 relative sites / 191,608 `.data.rel.ro` bytes to regex-syntax, 6,202 / 117,944 to grit-lib, and 3,925 / 55,376 to grit-cli. The only `.init_array` entries are Rust argv setup and the CRT frame registration. Clap subcommands are deferred, the schema registry is a `OnceLock`, tracing setup is gated by `GRIT_TRACE`, and jaq construction occurs only when filtering. No registration removal or dependency-feature reduction is justified by these data. Unicode regex/filter functionality remains intact.

`LD_DEBUG` median relocation cycles (stock / RELR / Git) are separate from wall time:

| Profile / command | Relocation cycles: stock / RELR / Git |
| --- | ---: |
| release / branch | 90054 / 87066 / 24828 |
| release / status | 87246 / 86952 / 24544 |
| release / resolve | 86450 / 84660 / 23817 |
| release / help | 95941 / 91092 / 27562 |
| dist / branch | 55106 / 54334 / 23084 |
| dist / status | 48866 / 51771 / 21356 |
| dist / resolve | 49384 / 51524 / 21896 |
| dist / help | 56049 / 54905 / 24299 |

Loader-only relocation cycles are mostly unchanged on dist. The end-to-end improvement cannot be attributed solely to faster loader relocation processing: metadata size and binary layout both change. Native fault samples are stored with every scenario; no major faults were observed. The exact 300-invocation `perf record -g -F 999` reproduction was attempted, but this VM denies perf events (`perf_event_paranoid=4`); no loader task-clock percentage is claimed.

## Portability decision

Both the published v0.5.21 x86_64 GNU artifact and current stock dist pass help, branch, status and resolve on glibc 2.34 (Rocky Linux 9). Their `GLIBC_2.39` requirement is weak, and the highest strong GNU symbol requirement is 2.34. The RELR artifacts pass those commands on glibc 2.36 (Debian bookworm) and fail on 2.34 with missing `GLIBC_ABI_DT_RELR`. Enabling RELR on the existing GNU artifact would raise its working minimum. The release workflow currently enforces the verified 2.34 contract; packing awaits an explicit decision on a newer minimum or a separate artifact. Apple, Windows, static musl and GNU aarch64 are not changed or claimed verified for RELR.

glibc documents the new format in its [2.36 release notes](https://raw.githubusercontent.com/bminor/glibc/glibc-2.36/NEWS); GNU ld documents [the flag and ABI dependency](https://sourceware.org/binutils/docs/ld/Options.html). Container digests and runtime results are in the ELF report. Four actual upload-pack process mappings per artifact had distinct base addresses with `randomize_va_space=2`, unchanged.

## Validation

- `cargo test -p grit-lib -p grit-cli`, before and after: compile fails at the unchanged `history_injected_ref_store.rs:45` (`Signature: From<&str>` not implemented).
- `cargo test -p grit-lib --lib`: before 2,617 passed / 3 failed / 3 ignored; after 2,620 passed / 0 failed / 3 ignored. The three baseline reset/fsck missing-object failures are intermittent and no library code changed.
- Candidate-linked `help_descriptions`, `output_snapshots`, `json_output`, `markdown_output`: stock release, RELR release and RELR dist each have 220 passes and the same 1 pre-existing JSON test failure (`bad_filter_and_missing_revision_errors_are_clean`, unborn log fails before filter syntax validation). All 125 snapshots, 19 Markdown cases and the help description test pass.
- `cargo test -p grit-lib --features http-ureq --test transport_http --test pack_ingest_roundtrip --test repack_roundtrip -- --nocapture`: 46 passed / 0 failed / 5 existing ignored (25 pack ingest, 12 repack, 9 smart-HTTP); the server was built first and no `SKIP` occurred. These exercise system-Git format/fsck and fetch/push round trips.
- ELF guard: 2 Rust linker-probe tests pass. The explicit artifact contract fails on stock (expected missing RELR), passes on release RELR and dist RELR. Static and runtime checks preserve PIE, GNU_RELRO/BIND_NOW, normal interpreter/dependencies and ASLR.
- `make gate`: fmt, workspace clippy, rustdoc with warnings denied and public-module doctests pass. Workspace test compilation exhausts the 145 GiB disk with debug executables (ENOSPC/linker bus errors); interrupted at that point. Disposable test executables were removed. The full lib/CLI command independently reproduces its source compile blocker. No test is weakened or skipped to green the gate.
- Docs check: 93 pages / 50 commands / 7 blog posts pass.

Reproduction commands are in `content/docs/benchmarking.md` and `TESTING.md`. Run the oldest-loader Docker checks from the release workflow before enabling any packing; inspecting only the highest GLIBC version string is insufficient.

## Remaining decision and follow-ups

Choose whether to preserve the existing GNU glibc 2.34 artifact and add a separate
RELR artifact for 2.36+, explicitly raise the GNU minimum to 2.36, or retain the
current release and leave RELR as an opt-in source build. No packing will be
silently enabled. A final enablement commit should close #2028 only after this
choice is implemented and verified. The source compilation blocker and baseline
JSON/reset failures are separate work; a clean full gate must be repeated once
those are resolved, with enough disk or test debug-info size reduced locally.
