#!/bin/sh
test_description='grit option parsing edge cases

Tests how grit handles common CLI patterns: unknown flags, --help,
ambiguous args, -C directory switching, --git-dir, double-dash
separators, and combined short options.'

. ./test-lib.sh

# ── Setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup: create a basic repo' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "content" >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

# ── --help for all major commands ────────────────────────────────────────────

test_expect_success 'grit --help succeeds' '
	git --help >out 2>&1 &&
	grep -i "usage" out
'

test_expect_success 'grit log --help succeeds' '
	git log --help >out 2>&1 &&
	grep -i "usage" out
'

test_expect_success 'grit diff --help succeeds' '
	git diff --help >out 2>&1 &&
	grep -i "usage" out
'

test_expect_success 'grit branch --help succeeds' '
	git branch --help >out 2>&1 &&
	grep -i "usage" out
'

test_expect_success 'grit tag --help succeeds' '
	git tag --help >out 2>&1 &&
	grep -i "usage" out
'

test_expect_success 'grit status --help succeeds' '
	git status --help >out 2>&1 &&
	grep -i "usage" out
'

# ── Unknown subcommand ───────────────────────────────────────────────────────

test_expect_success 'unknown subcommand fails' '
	test_must_fail git nosuchcmd 2>err &&
	grep -i "unrecognized" err
'

test_expect_success 'unknown subcommand suggests similar' '
	test_must_fail git stauts 2>err &&
	grep -i "similar" err || grep -i "status" err
'

# ── Unknown flags ────────────────────────────────────────────────────────────

test_expect_success 'log with unknown flag fails' '
	cd repo &&
	test_must_fail git log --bogus-flag 2>err
'

test_expect_success 'diff with unknown flag fails' '
	cd repo &&
	test_must_fail git diff --bogus-flag 2>err
'

test_expect_success 'branch with unknown flag fails' '
	cd repo &&
	test_must_fail git branch --bogus-flag 2>err
'

# ── -C directory switching ───────────────────────────────────────────────────

test_expect_success '-C switches to directory' '
	git -C repo rev-parse HEAD >out &&
	test -s out
'

test_expect_success '-C with log' '
	git -C repo log --oneline -n 1 >out &&
	grep "initial" out
'

test_expect_success '-C with status' '
	git -C repo status --porcelain >out
'

test_expect_success '-C with nonexistent dir fails' '
	test_must_fail git -C /no/such/dir status 2>err
'

# ── --git-dir ────────────────────────────────────────────────────────────────

test_expect_success '--git-dir can specify .git location' '
	git --git-dir=repo/.git rev-parse HEAD >out &&
	test -s out
'

test_expect_success '--git-dir with log' '
	git --git-dir=repo/.git log --oneline -n 1 >out &&
	grep "initial" out
'

# ── Double-dash separator ────────────────────────────────────────────────────

test_expect_failure 'diff uses -- to separate revisions from paths (not yet supported)' '
	cd repo &&
	echo "modified" >file.txt &&
	git diff -- file.txt >out &&
	grep "file\.txt" out &&
	git checkout -- file.txt
'

test_expect_failure 'log uses -- for path limiting (not yet supported)' '
	cd repo &&
	git log --oneline -- file.txt >out &&
	grep "initial" out
'

# ── -n / --max-count for log ────────────────────────────────────────────────

test_expect_success 'setup: add more commits for count tests' '
	cd repo &&
	echo "two" >file2.txt && git add file2.txt && git commit -m "second" &&
	echo "three" >file3.txt && git add file3.txt && git commit -m "third" &&
	echo "four" >file4.txt && git add file4.txt && git commit -m "fourth"
'

test_expect_success 'log -n 1 shows exactly one commit' '
	cd repo &&
	git log --oneline -n 1 >out &&
	test_line_count = 1 out
'

test_expect_success 'log -n 2 shows exactly two commits' '
	cd repo &&
	git log --oneline -n 2 >out &&
	test_line_count = 2 out
'

test_expect_success 'log --max-count=1 works same as -n 1' '
	cd repo &&
	git log --oneline --max-count=1 >out &&
	test_line_count = 1 out
'

test_expect_success 'log -n 0 shows no commits' '
	cd repo &&
	git log --oneline -n 0 >out &&
	test_must_be_empty out
'

# ── --version ────────────────────────────────────────────────────────────────

test_expect_success 'grit --version prints version info' '
	git --version >out 2>&1 &&
	test -s out
'

test_expect_success 'grit -V prints version info' '
	git -V >out 2>&1 &&
	test -s out
'

# ── rev-parse edge cases ────────────────────────────────────────────────────

test_expect_success 'rev-parse HEAD works' '
	cd repo &&
	git rev-parse HEAD >out &&
	test $(wc -c <out) -gt 39
'

test_expect_success 'rev-parse --git-dir shows .git' '
	cd repo &&
	git rev-parse --git-dir >out &&
	grep "\.git" out
'

test_expect_success 'rev-parse --is-bare-repository returns false' '
	cd repo &&
	git rev-parse --is-bare-repository >out &&
	grep "false" out
'

test_expect_success 'rev-parse --show-toplevel shows repo root' '
	cd repo &&
	git rev-parse --show-toplevel >out &&
	test -s out
'

test_expect_success 'rev-parse with invalid ref fails' '
	cd repo &&
	test_must_fail git rev-parse does-not-exist 2>err
'

test_done
