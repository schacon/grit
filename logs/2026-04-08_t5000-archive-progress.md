# t5000-tar-tree / archive progress

## Done this session

- `git clone --template= --bare`: minimal git-dir layout via `init_bare_clone_minimal` (no pre-created `info/`) so `mkdir bare.git/info` matches Git.
- `archive`: manual argv ordering (`--prefix` / `--add-file`), `export-ignore` / `export-subst`, pathspecs + `:(attr:...)`, tar pax global `comment`, gzip formats, tar config filters, `--remote` via pkt-line + sideband.
- `upload-archive`: pkt-line arguments, ACK+flush, sideband-wrapped archive output; reuses `archive_bytes_for_repo`.
- `pathspec`: `:(attr:name)` support via `matches_pathspec_for_object`.
- `crlf`: `export-ignore` / `export-subst` on `FileAttrs`, `path_has_gitattribute`.
- `pkt_line`: `write_sideband_channel1_64k`, `decode_sideband_primary`.

## Still failing (partial t5000 run)

- Ordering of tar entries vs reference `b.tar` (validate filenames/contents for several check_tar blocks).
- `git archive --remote=foo` from subdirectory (remote URL `.` resolution).
- Remote `.tgz` output compare.
- `clients cannot access unreachable commits` (remote must reject unreachable refs).
- `archive --list` tar filter names from config (`tar.tar.foo` → list as `tar.foo`).

## Note

Full `t5000-tar-tree.sh` can exceed 120s on slow runs (huge blob tests); use a higher timeout when refreshing CSV.
