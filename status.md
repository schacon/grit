# Grit Status

Last reviewed: 2026-04-29.

This is a directional status snapshot based on the current Rust module layout,
`plan.md`, `progress.md`, `test-results.md`, and `data/test-files.csv`. The
sources do not perfectly agree: `progress.md` currently reports 381 completed
plan tasks, 6 in progress, and 385 remaining, while `data/test-files.csv` has a
larger and newer-looking harness view with 396 fully passing files and
21,982/45,999 tests passing. Where they differ, this document treats the CSV as
the more granular coverage source and the plan as the intended work order.

## Coverage Snapshot

| Area | CSV pass rate | Fully passing files | What that means |
| --- | ---: | ---: | --- |
| t0 basic/setup | 84.2% | 41 | Strong base behavior, with remaining gaps in init, environment, path utils, cache-tree, encoding/conversion, and some harness/tool helpers. |
| t1 plumbing | 41.6% | 79 | Many core plumbing commands work, but the area is broad and still has major open slices in refs, cat-file/hash-object edges, sparse checkout, commit graph, reftable, and low-level repo setup. |
| t2 index/checkout | 48.5% | 27 | Basic checkout, switch, checkout-index, add/update-index paths are useful; submodules, sparse index, worktrees, overwrite safety, patch modes, and parallel checkout remain rough. |
| t3 core commands | 70.7% | 50 | The best-covered porcelain-ish area: ls-files, notes slices, rebase/cherry-pick slices, history, replay, and stash pieces are solid, but broad branch/rebase/stash/rm/add-interactive coverage is incomplete. |
| t4 diff/apply/log/am | 45.9% | 43 | Many high-value diff/apply features work, but the overall diff/log/format-patch/am surface is too large to call complete. |
| t5 transport/storage | 56.7% | 61 | Credentials, protocol policy, several HTTP/SSH push/fetch cases, packs, reverse indexes, and local clone pieces are strong; shallow, partial clone, generic fetch/push/remote, MIDX/bitmap, and archive edges remain open. |
| t6 rev/merge machinery | 54.3% | 35 | Merge-base, many merge recursive/ort cases, pathspecs, and rev-list basics work; advanced rev-list filters, merge rename directories, replace/describe/bisect/tracking stats are not mature. |
| t7 porcelain | 52.1% | 36 | Selected commit/status/reset/submodule/merge hooks and formats work; general daily porcelain still has many missing edge cases. |
| t8 blame/misc | 24.1% | 13 | Cat-file filters/textconv and simple blame cases work; full blame and last-modified behavior are still early. |
| t9 contrib/other | 18.0% | 11 | Mostly not a focus yet: completion, bash prompt, send-email, scalar, fast-import/export edges, and shell pieces remain sparse. |

## grit-lib Is Good At

`grit-lib` has a real core engine rather than a thin command-only
implementation. It is strongest in the areas that many commands share:

- Repository discovery and layout basics: `.git` files, bare/safe-bare checks,
  shared repository modes, worktree cwd handling, and several init/reinit paths.
- Object and index fundamentals: object IDs and object parsing, loose object
  storage, Git index read/write, tree writing, resolve-undo metadata, intent-to-add
  entries, and enough index semantics to support many checkout/add/reset flows.
- Config, attributes, ignore, pathspec, quoting, CRLF, and whitespace handling:
  attributes and auto-CRLF have substantial passing coverage, and pathspec
  matching includes wildcards, magic, exclusions, cwd-relative handling, and
  case-insensitive matching across several commands.
- References and reflogs: check-ref-format, symbolic refs, packed/loose refs,
  many update-ref flows, worktree/submodule refs, reflog display/walk/write
  basics, hide-refs, namespaces, and raw-ref handling are all meaningful.
- Revision parsing and walking: many `rev-parse` shorthands, reflog selectors,
  merge-base, grafts, path-limited walks, `rev-list` basics, object traversal,
  and Bloom-filter-backed log paths exist.
- Diff and apply foundations: rename/copy detection, several diff algorithms,
  patch-id, diffstat, whitespace checking/fixing, binary patches, textconv
  cache, combined-diff pieces, and a large amount of Git patch parsing/applying.
- Merge primitives: merge-base, merge-file, merge-tree helpers, recursive/ort
  style tree merges, conflict markers, renormalization, subtree strategy,
  criss-cross cases, and many file/directory conflict cases are implemented.
- Pack and transport primitives: pack/index parsing, delta handling,
  pack-objects slices, reverse indexes, MIDX groundwork, pkt-line handling,
  upload/receive pack support, shallow/promisor markers, and credential-aware
  HTTP/SSH transport pieces.
- Submodule foundations: `.gitmodules` parsing, active-state checks, gitdir
  migration/config, default remote resolution, and path/url handling support
  several CLI submodule operations.

For library consumers, the useful parts are the low-level Git data model and
shared behavior modules. The public API surface is broad and still looks more
like an internal command engine than a carefully stabilized external library.

## grit-lib Is Not Yet Good At

The weaker library areas are mostly advanced Git machinery and broad edge-case
surfaces:

- SHA-256 and alternate object-format support are still limited. Many code paths
  explicitly accept only SHA-1 or fail on unsupported object formats.
- Reftable, ref backend migration, ref fsck, bad-ref recovery, and some reflog
  expiration/pruning paths remain incomplete.
- Cache-tree, split-index, sparse-index compatibility, untracked cache,
  fsmonitor, racy timestamp behavior, and parallel checkout are partial.
- Commit-graph, Bloom, MIDX, bitmap, reverse-index, and pack reuse support is
  uneven: some focused suites pass, but verify/expire/repack, bitmap reuse,
  stdin pack modes, cruft packs, and multi-pack bitmap cases remain open.
- Revision filtering is not complete: object filters, missing-object handling,
  glob/exclude-hidden combinations, ancestry-path, bisect, TREESAME
  simplification, line-log, pretty formats, and mailmap are still partial.
- Merge behavior is strong for many individual cases but not complete across
  directory rename detection, submodule conflicts, some criss-cross corners,
  filemode conflicts, rerere, merge-tree `--write-tree`, and octopus/custom
  strategy edges.
- Conversion/filter-process/working-tree-encoding behavior is not as mature as
  CRLF and attributes. External filters and encoding edge cases need more work.
- Submodule internals work in important focused paths, but recursive fetch/push,
  update, sync, foreach, nested shallow/partial cases, and mixed ref storage
  still have many failures.
- Error compatibility is mixed: many diagnostics are Git-like, but the project
  still has command-specific error shaping rather than a uniformly polished
  library error contract.

## grit-lib Is Nearly Or Entirely Unimplemented

These are the areas that still look absent, stubbed, or represented only by
minimal scaffolding:

- Full SHA-256 repository operation and cross-object-format compatibility.
- Full reftable read/write behavior and migration between ref backends.
- Full sparse-index, split-index, untracked-cache, fsmonitor, and cache-tree
  behavior.
- Full commit-graph and bitmap lifecycle management, including advanced MIDX
  bitmap reuse and cruft/prune interactions.
- Direct path-walk API coverage, upstream ahead/behind tracking stats, and some
  low-level test-tool-only APIs.
- Full rerere, directory-rename merge, and advanced merge-tree write-tree
  behavior.
- Complete partial clone and promisor lazy-fetch behavior outside the focused
  cases that already pass.

## grit-cli Is Good At

The CLI has native command modules for most Git subcommands. The most usable
areas today are the commands backed by focused passing upstream suites:

- Basic command infrastructure: `git -C`, global config options, help/list-cmds
  slices, aliases/autocorrect, version, var, stripspace, usage parsing, and
  several test-tool helpers.
- Repository setup and simple plumbing: `init` slices, `repo info`,
  `check-ref-format`, `symbolic-ref`, `show-ref`, `update-ref`, `commit-tree`,
  `write-tree`, `read-tree` three-way/prefix/reset cases, `mktree`,
  `hash-object`/`cat-file` slices, and `fsck` buffer checks.
- Working tree and index basics: `add` pathspecs, `add -N`, add update/all
  slices, `checkout-index`, many `switch` and checkout branch/path cases,
  `update-index` bits, `reset --patch`, `rm` slices, and basic `mv`.
- Diff/apply/log slices: `diff` blob/tree/worktree comparisons, many rename and
  mode cases, `patch-id`, large pieces of `apply` including binary and symlink
  safety, selected `format-patch`/`am` paths, log graph/pickaxe/Bloom cases, and
  combined/remerge diff slices.
- Commit/sequencer slices: selected commit authorship, verbose template,
  pathspec-file, patch commit, cherry-pick conflict/signoff/`-x` cases, many
  rebase option/topology/autosquash/autostash slices, `replay`, and the custom
  `history` command's reword/split tests.
- Notes and stash slices: many notes add/merge/fanout/manual resolve cases, and
  stash include-untracked/pathspec/show pieces.
- Transport and credentials: credential, credential-store, credential-cache,
  smart HTTP auth/proxy/redaction, protocol allow/deny policy, local/SSH/HTTP
  upload-pack and receive-pack slices, push mirror/default/upstream pieces,
  local clone/clone branch/reference/dirname/revision slices, and several pack
  commands.
- Submodule focused paths: active checks, path/url handling, absorbgitdirs,
  sparse `.gitmodules`, summary/add slices, reference clones, protocol policy,
  default remote, and selected update/rebase/stash interactions.
- Status/grep/blame selected formats: porcelain v2, ignored modes, sparse grep,
  cat-file textconv/filters, and basic blame formats/corner cases have useful
  coverage.

In short: the CLI can perform many real Git workflows in simple or specifically
covered shapes. It should not yet be treated as a drop-in Git replacement for
arbitrary repositories.

## grit-cli Is Not Yet Good At

The CLI is weakest where Git's porcelain surface is broad, interactive, or full
of long-tail compatibility rules:

- Daily porcelain is uneven. `commit`, `status`, `branch`, `checkout`, `reset`,
  `restore`, `clean`, `tag`, `grep`, `show`, `submodule`, `remote`, `pull`,
  `fetch`, and `push` all have useful passing slices but still many failing
  upstream cases.
- Interactive commands are limited. `add -i`, `checkout --patch`,
  `restore --patch`, rebase interactive fixup/merge flows, stash patch, and
  clean interactive remain incomplete or fragile.
- Transport is not general-purpose yet. Smart HTTP/SSH and credentials have good
  focused coverage, but shallow fetch/push, partial clone, recursive submodule
  transport, refspec edge cases, namespaces, remote helpers, ext transport,
  protocol v2 push, and many remote porcelain details are incomplete.
- Storage maintenance commands are partial: `gc`, `repack`, `prune`,
  `commit-graph`, `multi-pack-index`, `pack-objects`, bitmaps, cruft packs, and
  commit-graph split chains are not mature enough for heavy maintenance use.
- Diff/log/am/format-patch are useful but incomplete across broad formatting,
  external diff, word diff, stat/dirstat variants, pretty formats, mailmap,
  line-log, corrupt input diagnostics, submodule diff formats, and format-patch
  mail edge cases.
- Merge/sequencer porcelain is mixed: many merge core cases work, but merge
  abort/state, custom strategies, octopus, rerere, directory rename conflicts,
  submodule conflicts, revert/cherry-pick sequence edges, and rebase
  `--rebase-merges`/fork-point/continue/abort families still have gaps.
- Signed object workflows are not complete. Signed commits/tags, SSH signing,
  push signing, and verification are only partially covered.
- Blame, describe, bisect, replace, shortlog, interpret-trailers, mailinfo,
  mailsplit, request-pull, and maintenance/scalar are at best partially usable.

## grit-cli Is Nearly Or Entirely Unimplemented

Some commands or modes are explicitly stubbed or close to placeholder level:

- `http-fetch` is documented in the command source as a dumb-HTTP stub.
- `imap-send` only handles the empty-input compatibility case; real IMAP
  delivery is not implemented.
- `merge-tree --trivial-merge` returns a not-implemented fatal error.
- `restore --patch --staged` returns a not-implemented error.
- `commit-graph write --stdin-packs` returns a not-implemented error.
- Smart HTTP push and SSH push over protocol v2 return not-implemented errors.
- Push signing is accepted by the parser but marked as not implemented.
- Full Git shell/send-email/completion/bash-prompt/scalar ecosystems have little
  useful implementation compared with core Git behavior.
- Several t9/contrib areas are mostly out of focus: bash completion, send-email,
  scalar clone, fast-import/export edge modes, and shell workflows have low or
  zero meaningful coverage.

## Practical Bottom Line

`grit-lib` is already a substantial Git-compatible engine for SHA-1 repositories,
especially around objects, indexes, refs, pathspecs, diff/apply, many merge
cases, and selected transport/storage primitives. The CLI is capable in many
targeted workflows and has a surprisingly wide command surface, but reliability
is currently best described as "strong in covered slices, not yet generally
Git-compatible."

The safest heavy-use areas are focused plumbing, selected diff/apply flows,
selected merge/rebase/cherry-pick cases, credential/auth/protocol policy, and
some pack/clone/fetch/push scenarios. The riskiest areas are broad porcelain,
interactive workflows, advanced transport/storage maintenance, submodule
recursion, sparse-index/cache machinery, SHA-256, and the contrib-style command
families.
