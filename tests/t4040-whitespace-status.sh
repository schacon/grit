#!/bin/sh
#
# Tests for whitespace changes and diff exit status / output modes.

test_description='whitespace changes and diff exit status'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	printf "a\nb\nc\n" >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

# ---------------------------------------------------------------------------
# diff --exit-code
# ---------------------------------------------------------------------------
test_expect_success 'diff --exit-code returns 0 when no changes' '
	cd repo &&
	git diff --exit-code
'

test_expect_success 'diff --exit-code returns 1 when changes exist' '
	cd repo &&
	printf "a\nb\nc\nd\n" >file.txt &&
	test_must_fail git diff --exit-code &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff --exit-code returns 1 for whitespace-only change' '
	cd repo &&
	printf "a \nb\nc\n" >file.txt &&
	test_must_fail git diff --exit-code &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff --exit-code returns 1 for trailing whitespace addition' '
	cd repo &&
	printf "a\t\nb\nc\n" >file.txt &&
	test_must_fail git diff --exit-code &&
	printf "a\nb\nc\n" >file.txt
'

# ---------------------------------------------------------------------------
# diff --quiet
# ---------------------------------------------------------------------------
test_expect_success 'diff --quiet returns 0 when no changes' '
	cd repo &&
	git diff --quiet
'

test_expect_success 'diff --quiet returns 1 when changes exist' '
	cd repo &&
	printf "a\nb\nc\nd\n" >file.txt &&
	test_must_fail git diff --quiet &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff --quiet produces no output' '
	cd repo &&
	printf "a\nb\nmodified\n" >file.txt &&
	git diff --quiet >out 2>&1 || true &&
	! test -s out &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff --quiet returns 1 for whitespace-only change' '
	cd repo &&
	printf "a \nb\nc\n" >file.txt &&
	test_must_fail git diff --quiet &&
	printf "a\nb\nc\n" >file.txt
'

# ---------------------------------------------------------------------------
# diff --name-only
# ---------------------------------------------------------------------------
test_expect_success 'diff --name-only shows modified file' '
	cd repo &&
	printf "changed\n" >file.txt &&
	git diff --name-only >out &&
	grep "file.txt" out &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff --name-only with multiple files' '
	cd repo &&
	echo "extra" >other.txt &&
	git add other.txt &&
	git commit -m "add other" &&
	printf "changed\n" >file.txt &&
	printf "changed\n" >other.txt &&
	git diff --name-only >out &&
	grep "file.txt" out &&
	grep "other.txt" out &&
	printf "a\nb\nc\n" >file.txt &&
	echo "extra" >other.txt
'

test_expect_success 'diff --name-only empty when no changes' '
	cd repo &&
	git diff --name-only >out &&
	! test -s out
'

# ---------------------------------------------------------------------------
# diff --name-status
# ---------------------------------------------------------------------------
test_expect_success 'diff --name-status shows M for modified file' '
	cd repo &&
	printf "changed\n" >file.txt &&
	git diff --name-status >out &&
	grep "^M" out &&
	grep "file.txt" out &&
	printf "a\nb\nc\n" >file.txt
'

# ---------------------------------------------------------------------------
# diff --numstat
# ---------------------------------------------------------------------------
test_expect_success 'diff --numstat shows additions and deletions' '
	cd repo &&
	printf "a\nb\nc\nd\n" >file.txt &&
	git diff --numstat >out &&
	grep "file.txt" out &&
	# Should show 1 addition, 0 deletions or similar
	test -s out &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff --numstat empty when no changes' '
	cd repo &&
	git diff --numstat >out &&
	! test -s out
'

# ---------------------------------------------------------------------------
# diff --stat
# ---------------------------------------------------------------------------
test_expect_success 'diff --stat shows summary' '
	cd repo &&
	printf "changed\nb\nc\n" >file.txt &&
	git diff --stat >out &&
	grep "file.txt" out &&
	grep "changed" out &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff --stat empty when no changes' '
	cd repo &&
	git diff --stat >out &&
	! test -s out
'

# ---------------------------------------------------------------------------
# diff --cached variants
# ---------------------------------------------------------------------------
test_expect_success 'diff --cached --exit-code returns 0 with no staged changes' '
	cd repo &&
	git diff --cached --exit-code
'

test_expect_success 'diff --cached --exit-code returns 1 with staged changes' '
	cd repo &&
	printf "staged\n" >file.txt &&
	git add file.txt &&
	test_must_fail git diff --cached --exit-code &&
	git reset HEAD file.txt &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff --cached --name-only shows staged files' '
	cd repo &&
	printf "staged\n" >file.txt &&
	git add file.txt &&
	git diff --cached --name-only >out &&
	grep "file.txt" out &&
	git reset HEAD file.txt &&
	printf "a\nb\nc\n" >file.txt
'

# ---------------------------------------------------------------------------
# Whitespace-specific diff behavior
# ---------------------------------------------------------------------------
test_expect_success 'diff detects trailing space addition' '
	cd repo &&
	printf "a \nb\nc\n" >file.txt &&
	git diff >out &&
	test -s out &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff detects trailing tab addition' '
	cd repo &&
	printf "a\t\nb\nc\n" >file.txt &&
	git diff >out &&
	test -s out &&
	printf "a\nb\nc\n" >file.txt
'

test_expect_success 'diff detects space-to-tab change' '
	cd repo &&
	printf "a\n b\nc\n" >file.txt &&
	git add file.txt &&
	git commit -m "indented" &&
	printf "a\n\tb\nc\n" >file.txt &&
	git diff >out &&
	test -s out &&
	git checkout file.txt
'

test_expect_success 'diff detects blank line addition' '
	cd repo &&
	printf "a\n\nb\nc\n" >file.txt &&
	git diff >out &&
	test -s out &&
	printf "a\nb\nc\n" >file.txt
'

test_done
