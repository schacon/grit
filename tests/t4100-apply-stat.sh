#!/bin/sh
# Test diff --stat, --numstat, --name-only, --name-status comprehensively.
# grit does not have apply, so we focus on stat-related diff output formats.

test_description='diff --stat, --numstat, --name-only, --name-status'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo with initial commit' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "line1" >file.txt &&
	grit add file.txt &&
	grit commit -m "initial"
'

# ── single file modification ──

test_expect_success 'diff --stat for single file change' '
	cd repo &&
	echo "line2" >>file.txt &&
	grit add file.txt &&
	grit diff --stat --cached >actual &&
	grep "file.txt" actual &&
	grep "1 insertion" actual
'

test_expect_success 'diff --numstat for single file change' '
	cd repo &&
	grit diff --numstat --cached >actual &&
	# format: added<TAB>removed<TAB>filename
	grep "^1	0	file.txt" actual
'

test_expect_success 'diff --name-only for single file change' '
	cd repo &&
	grit diff --name-only --cached >actual &&
	test "$(cat actual)" = "file.txt"
'

test_expect_success 'diff --name-status for single file change' '
	cd repo &&
	grit diff --name-status --cached >actual &&
	grep "^M" actual &&
	grep "file.txt" actual
'

test_expect_success 'commit single change' '
	cd repo &&
	grit commit -m "add line2"
'

# ── multiple file changes ──

test_expect_success 'setup multiple files' '
	cd repo &&
	echo "aaa" >a.txt &&
	echo "bbb" >b.txt &&
	echo "ccc" >c.txt &&
	grit add a.txt b.txt c.txt &&
	grit commit -m "add a b c"
'

test_expect_success 'diff --stat with multiple files changed' '
	cd repo &&
	echo "aaa2" >>a.txt &&
	echo "bbb2" >>b.txt &&
	grit diff --stat >actual &&
	grep "a.txt" actual &&
	grep "b.txt" actual &&
	grep "2 files changed" actual
'

test_expect_success 'diff --numstat with multiple files' '
	cd repo &&
	grit diff --numstat >actual &&
	grep "a.txt" actual &&
	grep "b.txt" actual &&
	test_line_count = 2 actual
'

test_expect_success 'diff --name-only with multiple files' '
	cd repo &&
	grit diff --name-only >actual &&
	grep "a.txt" actual &&
	grep "b.txt" actual &&
	! grep "c.txt" actual
'

test_expect_success 'diff --name-status with multiple files' '
	cd repo &&
	grit diff --name-status >actual &&
	grep "M.*a.txt" actual &&
	grep "M.*b.txt" actual
'

test_expect_success 'commit multiple changes' '
	cd repo &&
	grit add a.txt b.txt &&
	grit commit -m "update a b"
'

# ── new file ──

test_expect_success 'diff --stat for new file (cached)' '
	cd repo &&
	echo "new content" >new.txt &&
	grit add new.txt &&
	grit diff --stat --cached >actual &&
	grep "new.txt" actual &&
	grep "1 insertion" actual
'

test_expect_success 'diff --name-status shows A for new file' '
	cd repo &&
	grit diff --name-status --cached >actual &&
	grep "^A" actual &&
	grep "new.txt" actual
'

test_expect_success 'diff --numstat for new file' '
	cd repo &&
	grit diff --numstat --cached >actual &&
	grep "^1	0	new.txt" actual
'

test_expect_success 'commit new file' '
	cd repo &&
	grit commit -m "add new.txt"
'

# ── deleted file ──

test_expect_success 'diff --stat for deleted file (cached)' '
	cd repo &&
	grit rm c.txt &&
	grit diff --stat --cached >actual &&
	grep "c.txt" actual &&
	grep "1 deletion" actual
'

test_expect_success 'diff --name-status shows D for deleted file' '
	cd repo &&
	grit diff --name-status --cached >actual &&
	grep "^D" actual &&
	grep "c.txt" actual
'

test_expect_success 'diff --numstat for deleted file' '
	cd repo &&
	grit diff --numstat --cached >actual &&
	grep "^0	1	c.txt" actual
'

test_expect_success 'commit deleted file' '
	cd repo &&
	grit commit -m "delete c.txt"
'

# ── between commits ──

test_expect_success 'diff --stat between two commits' '
	cd repo &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "c.txt" actual
'

test_expect_success 'diff --numstat between two commits' '
	cd repo &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "c.txt" actual
'

test_expect_success 'diff --name-only between two commits' '
	cd repo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	grep "c.txt" actual
'

test_expect_success 'diff --name-status between two commits' '
	cd repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "D.*c.txt" actual
'

# ── large changes ──

test_expect_success 'diff --stat with many lines changed' '
	cd repo &&
	for i in $(seq 1 100); do echo "line $i"; done >big.txt &&
	grit add big.txt &&
	grit commit -m "add big.txt" &&
	for i in $(seq 1 50); do echo "new line $i"; done >big.txt &&
	grit diff --stat >actual &&
	grep "big.txt" actual
'

test_expect_success 'diff --numstat with many lines' '
	cd repo &&
	grit diff --numstat >actual &&
	LINE=$(grep "big.txt" actual) &&
	# At least one side (added or deleted) should be nonzero
	ADDED=$(echo "$LINE" | awk "{print \$1}") &&
	DELETED=$(echo "$LINE" | awk "{print \$2}") &&
	test $(($ADDED + $DELETED)) -gt 0
'

test_expect_success 'commit big changes' '
	cd repo &&
	grit add big.txt &&
	grit commit -m "update big.txt"
'

# ── many files ──

test_expect_success 'diff --stat with many files' '
	cd repo &&
	for i in $(seq 1 10); do
		echo "content $i" >multi-$i.txt || return 1
	done &&
	grit add multi-*.txt &&
	grit commit -m "add multi files" &&
	for i in $(seq 1 10); do
		echo "changed $i" >multi-$i.txt || return 1
	done &&
	grit diff --stat >actual &&
	grep "10 files changed" actual
'

test_expect_success 'diff --numstat with many files' '
	cd repo &&
	grit diff --numstat >actual &&
	test_line_count = 10 actual
'

test_expect_success 'diff --name-only with many files sorted' '
	cd repo &&
	grit diff --name-only >actual &&
	test_line_count = 10 actual &&
	sort actual >sorted &&
	test_cmp actual sorted
'

test_expect_success 'commit multi files' '
	cd repo &&
	grit add multi-*.txt &&
	grit commit -m "update multi"
'

# ── empty diff ──

test_expect_success 'diff --stat with no changes is empty' '
	cd repo &&
	grit diff --stat >actual &&
	test_must_be_empty actual
'

test_expect_success 'diff --numstat with no changes is empty' '
	cd repo &&
	grit diff --numstat >actual &&
	test_must_be_empty actual
'

test_expect_success 'diff --name-only with no changes is empty' '
	cd repo &&
	grit diff --name-only >actual &&
	test_must_be_empty actual
'

# ── exit codes ──

test_expect_success 'diff --exit-code returns 0 when no changes' '
	cd repo &&
	grit diff --exit-code
'

test_expect_success 'diff --exit-code returns 1 when changes exist' '
	cd repo &&
	echo "trigger" >>file.txt &&
	test_must_fail grit diff --exit-code
'

test_expect_success 'diff --quiet suppresses output but returns exit code' '
	cd repo &&
	grit diff --quiet >actual 2>&1 || true &&
	test_must_be_empty actual
'

test_expect_success 'cleanup exit code test' '
	cd repo &&
	git checkout -- file.txt
'

# ── subdirectory paths ──

test_expect_success 'diff --stat with subdirectory files' '
	cd repo &&
	mkdir -p sub/deep &&
	echo "deep" >sub/deep/file.txt &&
	grit add sub/ &&
	grit commit -m "add subdir" &&
	echo "changed" >sub/deep/file.txt &&
	grit diff --stat >actual &&
	grep "sub/deep/file.txt" actual
'

test_expect_success 'diff --numstat with subdirectory' '
	cd repo &&
	grit diff --numstat >actual &&
	grep "sub/deep/file.txt" actual
'

test_expect_success 'diff --name-only with subdirectory' '
	cd repo &&
	grit diff --name-only >actual &&
	grep "sub/deep/file.txt" actual
'

test_expect_success 'commit subdir changes' '
	cd repo &&
	grit add sub/ &&
	grit commit -m "update subdir"
'

test_done
