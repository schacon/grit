#!/bin/sh
# Tests for add -u / --update: stage modifications and deletions of tracked files
# without adding new untracked files.

test_description='add -u (update tracked files)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ──────────────────────────────────────────────────────────────

test_expect_success 'setup repo with tracked files' '
	grit init repo &&
	cd repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	echo "aaa" >tracked1.txt &&
	echo "bbb" >tracked2.txt &&
	echo "ccc" >tracked3.txt &&
	mkdir sub &&
	echo "ddd" >sub/tracked4.txt &&
	grit add . &&
	grit commit -m "initial"
'

# ── Basic: add -u stages modifications ────────────────────────────────

test_expect_success 'modify tracked file and run add -u' '
	cd repo &&
	echo "modified" >tracked1.txt &&
	grit add -u &&
	grit diff --cached --name-only >actual &&
	grep "tracked1.txt" actual
'

test_expect_success 'add -u does not stage untracked files' '
	cd repo &&
	echo "new" >untracked.txt &&
	grit add -u &&
	grit ls-files >indexed &&
	! grep "untracked.txt" indexed
'

test_expect_success 'untracked file still shows in status' '
	cd repo &&
	grit status >out &&
	grep "untracked.txt" out
'

# ── add -u stages deletions ───────────────────────────────────────────

test_expect_success 'delete tracked file and run add -u' '
	cd repo &&
	rm tracked2.txt &&
	grit add -u &&
	grit diff --cached --name-status >actual &&
	grep "^D" actual &&
	grep "tracked2.txt" actual
'

test_expect_success 'deleted file removed from index after add -u' '
	cd repo &&
	grit ls-files >indexed &&
	! grep "tracked2.txt" indexed
'

# ── Commit and verify state ───────────────────────────────────────────

test_expect_success 'commit after add -u' '
	cd repo &&
	rm -f untracked.txt &&
	grit commit -m "update and delete" &&
	grit diff --cached >cached &&
	test_line_count = 0 cached
'

# ── add -u with no changes is a no-op ─────────────────────────────────

test_expect_success 'add -u with no tracked-file changes is a no-op' '
	cd repo &&
	grit add -u &&
	grit diff --cached >cached_out &&
	test_line_count = 0 cached_out
'

# ── add -u stages changes in subdirectories ───────────────────────────

test_expect_success 'modify file in subdirectory' '
	cd repo &&
	echo "modified-sub" >sub/tracked4.txt &&
	grit add -u &&
	grit diff --cached --name-only >actual &&
	grep "sub/tracked4.txt" actual
'

test_expect_success 'commit subdir change' '
	cd repo &&
	grit commit -m "update subdir"
'

# ── add -u with multiple modifications ────────────────────────────────

test_expect_success 'multiple modifications staged by add -u' '
	cd repo &&
	echo "mod1" >tracked1.txt &&
	echo "mod3" >tracked3.txt &&
	echo "mod4" >sub/tracked4.txt &&
	grit add -u &&
	grit diff --cached --name-only >actual &&
	grep "tracked1.txt" actual &&
	grep "tracked3.txt" actual &&
	grep "sub/tracked4.txt" actual
'

test_expect_success 'commit multiple changes' '
	cd repo &&
	grit commit -m "multi update"
'

# ── add -u with mixed modifications and deletions ─────────────────────

test_expect_success 'mix of modification and deletion' '
	cd repo &&
	echo "changed again" >tracked1.txt &&
	rm sub/tracked4.txt &&
	echo "brand new" >another-new.txt &&
	grit add -u &&
	grit diff --cached --name-status >actual &&
	grep "M.*tracked1.txt" actual &&
	grep "D.*sub/tracked4.txt" actual
'

test_expect_success 'new file not staged by add -u' '
	cd repo &&
	grit ls-files >indexed &&
	! grep "another-new.txt" indexed
'

test_expect_success 'commit mixed changes' '
	cd repo &&
	rm -f another-new.txt &&
	grit commit -m "mix changes"
'

# ── add --update is the long form of -u ───────────────────────────────

test_expect_success 'add --update works same as -u' '
	cd repo &&
	echo "long-form" >tracked1.txt &&
	grit add --update &&
	grit diff --cached --name-only >actual &&
	grep "tracked1.txt" actual
'

test_expect_success 'commit long form' '
	cd repo &&
	grit commit -m "long form update"
'

# ── add -u --dry-run shows what would be added ────────────────────────

test_expect_success 'add -u --dry-run shows changes without staging' '
	cd repo &&
	echo "dry-run-change" >tracked1.txt &&
	grit add -u --dry-run >out 2>&1 &&
	grep "tracked1.txt" out
'

test_expect_success 'after dry-run, file is not actually staged' '
	cd repo &&
	grit diff --cached >cached &&
	test_line_count = 0 cached
'

# ── add -u -v shows verbose output ────────────────────────────────────

test_expect_success 'add -u -v stages and may show verbose info' '
	cd repo &&
	grit add -u -v >out 2>&1 &&
	grit diff --cached --name-only >actual &&
	grep "tracked1.txt" actual
'

test_expect_success 'commit verbose update' '
	cd repo &&
	grit commit -m "verbose update"
'

# ── Verify final state ────────────────────────────────────────────────

test_expect_success 'final repo has expected tracked files' '
	cd repo &&
	grit ls-files >indexed &&
	grep "tracked1.txt" indexed &&
	grep "tracked3.txt" indexed &&
	! grep "tracked2.txt" indexed &&
	! grep "tracked4.txt" indexed
'

test_done
