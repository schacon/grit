# PIE startup relocation investigation (#2028)

Current shipping policy: keep the original GNU asset as the installer default
on every glibc version. RELR remains a manual download for glibc 2.36+. The
review follow-up below supersedes automatic selection and earlier performance
claims for the final artifacts. Historical measurements retain their exact
source revisions.

Base: `f0e7cddd530a27366b74a4901b778395e404b458` (origin/main at checkout). Both variants use that exact source. Candidate changes only the CLI final link with `-Wl,-z,pack-relative-relocs`; the map option is diagnostic. No codegen units, LTO, panic policy, PIE, ASLR, features, library algorithms or CLI behavior change. The initial probe predates the shipping decision implemented in the addendum below.

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

Both the published v0.5.21 x86_64 GNU artifact and current stock dist pass help, branch, status and resolve on glibc 2.34 (Rocky Linux 9). Their `GLIBC_2.39` requirement is weak, and the highest strong GNU symbol requirement is 2.34. The RELR artifacts pass those commands on glibc 2.36 (Debian bookworm) and fail on 2.34 with missing `GLIBC_ABI_DT_RELR`. Enabling RELR on the existing GNU artifact would raise its working minimum. The initial workflow enforces the verified 2.34 contract; the shipping addendum below implements a separate artifact. Apple, Windows, static musl and GNU aarch64 are not changed or claimed verified for RELR.

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

## Separate shipping artifact (2026-10-27)

The maintainer chose to retain the glibc 2.34 baseline and add an optional
x86_64 GNU RELR asset for glibc 2.36+. The release workflow packages and uploads
`grit-x86_64-unknown-linux-gnu.tar.gz` on its unchanged build path before running
a CLI-only `cargo rustc` link for `grit-x86_64-unknown-linux-gnu-relr.tar.gz`.
No workspace profile, dependency feature or other target changes. Both archives
include the same completions. The original GNU asset remains the default and
fallback; stable/nightly installers choose RELR only after confident detection
with `getconf GNU_LIBC_VERSION`, or `ldd --version` when inconclusive. Unknown,
malformed and older versions use the baseline, as do failed optional downloads.

Fresh paired measurement on `aa1964221` CLI/library source, Linux x86_64,
4-vCPU Intel Xeon Platinum 8573C, 16 GiB RAM, Rust 1.99.0 / LLD 23.1.1,
glibc 2.39 and system Git 2.55.0. Both frozen binaries use exactly that source;
only the candidate CLI link flag differs. The original release/dist reports
above remain historical measurements on the earlier EPYC host. This follow-up
repeats shipping dist with five warmups and **60 rotated measured runs per tool**
on all 20 tiny/many-ref/wide/deep cases, with builds and tests stopped. Full
fork/exec/wait medians ± sample standard deviations and maximum per-child RSS:

| Shipping dist / command | Git ms | Stock ms | RELR ms | Git / stock / RELR peak KiB |
| --- | ---: | ---: | ---: | ---: |
| tiny / branch | 1.166 ± 0.043 | 1.968 ± 0.062 | 1.871 ± 0.050 | 4788 / 9952 / 9504 |
| tiny / status | 1.402 ± 0.049 | 2.359 ± 0.078 | 2.254 ± 0.063 | 5448 / 11804 / 11364 |
| tiny / resolve | 1.078 ± 0.043 | 2.051 ± 0.063 | 1.945 ± 0.065 | 4676 / 9764 / 9376 |
| tiny / help | 0.966 ± 0.035 | 2.911 ± 0.095 | 2.880 ± 0.119 | 4100 / 9172 / 8740 |
| tiny / log | 1.256 ± 0.058 | 2.134 ± 0.076 | 2.058 ± 0.127 | 5276 / 10720 / 10212 |
| many-refs / branch | 45.161 ± 1.332 | 11.632 ± 0.378 | 11.496 ± 0.403 | 19748 / 15456 / 14952 |
| wide / status | 12.261 ± 1.052 | 13.589 ± 0.514 | 13.367 ± 0.536 | 9384 / 15136 / 14692 |
| deep / log | 6.755 ± 0.148 | 9.768 ± 0.332 | 9.650 ± 0.173 | 10120 / 17072 / 16632 |

Tiny branch/status/resolve improve **4.4–5.1%**. Help's 1.1% difference is within
the sample spread. All 20 candidate medians are lower in this sitting; larger
many-ref branch, wide status and deep log improve 1.2–1.6%, with identical
stdout/stderr. Git branch still lists all branches while grit limits to 20.
Do not extrapolate one host's gain to every workload or claim a general RSS win.
Here the native-helper RSS peaks fall about 0.4–0.5 MiB; ten independent
`/usr/bin/time -v` samples put tiny branch/status/resolve/help median peak RSS at
9676/11380/9572/8984 KiB stock versus 9190/10874/9064/8548 KiB RELR
(Git: 4366/5020/4128/3576 KiB). The earlier dist layout did not consistently
reduce RSS.

Stock/RELR binaries are **24,451,840 / 23,997,288 bytes**. Relative relocation
metadata is **464,592 / 9,872 bytes**, with **19,185 identical pointer slots on
91 pages**. Text, rodata and data.rel.ro sizes are unchanged. No major faults
occurred. Loader-only median relocation cycles (30 runs) are 39,274.5 → 46,564.5
for branch, 39,276.5 → 47,533.5 for status, 38,932.5 → 46,489.5 for resolve,
and 40,331.5 → 50,534 for help. These increased while wall time fell: metadata
compression and layout change do not prove a faster relocation loop. The exact
300-invocation `perf record -g -F 999` attempt is again denied by
`perf_event_paranoid=4`; no task-clock attribution is inferred.

Raw wall/fault samples, variance, binary hashes/sections, independent GNU-time
RSS, loader observations, runtime mappings, image smokes and profiler refusal:
[`startup-relocations-shipping-dist.json`](startup-relocations-shipping-dist.json).

### Portability and validation

- Baseline passes the release guard (PIE, standard interpreter/dependencies,
  GNU_RELRO/BIND_NOW, non-executable stack, no RELR ABI) and help/branch/status/
  resolve on pinned Rocky Linux 9 / **glibc 2.34**.
- RELR passes the same hardening/runtime guard with `GLIBC_ABI_DT_RELR`, and
  those commands on pinned Debian bookworm / **glibc 2.36**. It is explicitly
  rejected by 2.34 with the expected missing ABI. Both old-loader runs emit
  existing weak `GLIBC_2.39` warnings but exit successfully on supported loaders.
- Four actual load addresses per artifact are distinct with ASLR setting `2`,
  unchanged. Both builds preserve Unicode literal JSON regex filtering and
  `GRIT_TRACE=1` output. Their text/rodata/table sizes remain identical.
- Rust installer regressions fail against the original scripts (0 passed,
  2 failed), and pass against both new scripts across supported/old/unknown/
  malformed versions, ldd fallback, absent optional assets, musl and aarch64.
  Final installer/ELF run: **4 passed**, 1 opt-in artifact test normally ignored;
  that artifact test is also explicitly run and passes for RELR. Existing
  Python installer tests: **3 passed**.
- `cargo test -p grit-lib -p grit-cli` before and after both stop at unchanged
  `grit-lib/tests/history_injected_ref_store.rs:45`: `Signature: From<&str>` is
  not implemented. No aggregate pass count is claimed for this command.
- `cargo test -p grit-lib --lib`: before **2634 passed / 4 failed / 3 ignored**;
  after **2638 passed / 0 failed / 3 ignored**. The baseline failures are the
  held-index-lock commit object count assertion and three reset/fsck missing-
  object cases; no library source or tests change.
- Actual stock and RELR dist each pass **220 CLI tests** (help descriptions,
  all 125 output snapshots, 75 JSON, 19 Markdown), with the same pre-existing
  `bad_filter_and_missing_revision_errors_are_clean` failure on unborn log.
- Pack ingestion/repack/smart-HTTP system-Git round trips: **46 passed**,
  5 existing ignored. The HTTP server was built and no test printed `SKIP`.
- After rebasing on current main, **`make gate` passes fmt, workspace Clippy,
  warnings-denied rustdoc and public-module doctests**, then stops at the same
  `history_injected_ref_store` compilation error (about 17 GiB remained).
  Main then advanced by one unrelated rebase-exec change. A second clean rebase
  onto **`3c1a41ef2`** repeats the gate: fmt, Clippy, rustdoc and public-module
  doctests pass again; workspace test linking fills the 145 GiB disk (267 MiB
  free) and fails with LLD bus errors. The unchanged Signature conversion error
  remains in that base. After this failed run, 24.1 GiB of disposable test
  executables are removed to save the report; no gate rerun, profile change,
  test weakening or compile workaround is applied. Measurements and focused
  tests above retain their exact `aa1964221` revision.
- Docs check: **93 pages / 50 commands / 7 blog posts pass**.

The implementation is ready for review, but **integration still requires a
passing make gate** after the unrelated compile blocker is fixed on main.
Do not land with a failing gate. The existing JSON error-ordering failure and
intermittent baseline commit/reset failures are separate follow-ups.

An additional filter smoke found that jaq Unicode property classes (`\p{L}`)
are unsupported in both stock and RELR, with identical errors. Unicode literal
regex filtering passes in both; this existing filter limitation is unchanged
and outside the relocation work.

## Review correction: stock installer default (2026-10-27)

The review measured the actual `3fcfb24e9` tip, rather than extrapolating the
`aa1964221` results. Same-source native dist medians from five warmups and 60
rotated measured runs were:

| Tiny command | Git ms | Stock dist ms | RELR dist ms | RELR change |
| --- | ---: | ---: | ---: | ---: |
| branch | 0.964554 | 1.515249 | 1.541162 | +1.71% |
| status | 1.148274 | 1.782791 | 1.811804 | +1.63% |
| resolve | 0.887730 | 1.545215 | 1.572446 | +1.76% |
| help | 0.808081 | 2.319787 | 2.335541 | +0.68% |

These are reviewer-supplied results on the 4-vCPU Xeon 8573C / glibc 2.39 host,
not a new run on this VM. Independent 120-sample randomized and CPU-pinned
repeats confirmed regressions, with all eight paired-bootstrap 95% intervals
above zero. The earlier reports remain valid for their recorded artifacts but
do not justify automatically selecting RELR for this tip.

Both stable and nightly installers now retain stock selection for every glibc
version, including 2.36+. The unused glibc version parser, RELR selection and
optional-download retry are removed. The separate x86_64 GNU RELR release asset
and both ELF/oldest-loader release checks remain available. RELR is a manual
opt-in with no promised startup gain; no library/CLI code, dependency features,
profiles, PIE or ASLR policy changes. Reconsider automatic selection only after
exact release artifacts show a repeatable benefit.
