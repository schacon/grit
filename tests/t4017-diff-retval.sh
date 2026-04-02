#!/bin/sh
# Ported subset from git/t/t4017-diff-retval.sh for diff-index return values.

test_description='diff-index exit status and quiet mode'

. ./test-lib.sh

make_commit () {
	msg=$1
	parent=${2-}
	tree=$(git write-tree) || return 1
	if test -n "$parent"
	then
		commit=$(printf '%s\n' "$msg" | git commit-tree "$tree" -p "$parent") || return 1
	else
		commit=$(printf '%s\n' "$msg" | git commit-tree "$tree") || return 1
	fi
	git update-ref HEAD "$commit" || return 1
	printf '%s\n' "$commit"
}

test_expect_success 'setup two commits with index at second commit' '
	git init repo &&
	cd repo &&
	printf "one\n" >a &&
	git update-index --add a &&
	c1=$(make_commit first) &&
	printf "two\n" >a &&
	printf "side\n" >b &&
	git update-index a &&
	git update-index --add b &&
	c2=$(make_commit second "$c1") &&
	test -n "$c1" &&
	test -n "$c2" &&
	printf "%s\n" "$c1" >c1 &&
	printf "%s\n" "$c2" >c2
'

test_expect_success 'diff-index --cached --exit-code succeeds when identical' '
	cd repo &&
	c2=$(cat c2) &&
	git diff-index --cached --exit-code "$c2"
'

test_expect_success 'diff-index --cached --exit-code fails when different' '
	cd repo &&
	c1=$(cat c1) &&
	test_must_fail git diff-index --cached --exit-code "$c1"
'

test_expect_success 'diff-index --quiet returns non-zero and no output' '
	cd repo &&
	c1=$(cat c1) &&
	test_must_fail git diff-index --quiet --cached "$c1" >quiet.out 2>/dev/null &&
	test ! -s quiet.out
'

test_expect_success 'pathspec limits exit-code checks' '
	cd repo &&
	c1=$(cat c1) &&
	test_must_fail git diff-index --cached --exit-code "$c1" -- b &&
	git diff-index --cached --exit-code "$c1" -- does-not-exist
'

# ---------------------------------------------------------------------------
# Additional tests from git/t/t4017-diff-retval.sh
# ---------------------------------------------------------------------------

test_expect_success 'diff-files --exit-code succeeds when worktree matches index' '
	cd repo &&
	git diff-files --exit-code
'

test_expect_success 'diff-files --exit-code fails when worktree differs' '
	cd repo &&
	echo 3 >>a &&
	test_must_fail git diff-files --exit-code
'

test_expect_success 'diff --quiet returns 0 when HEAD matches worktree' '
	cd repo &&
	git update-index a &&
	c3=$(make_commit third "$(cat c2)") &&
	printf "%s\n" "$c3" >c3 &&
	git diff --quiet
'

test_expect_success 'diff --quiet returns 1 for HEAD^ HEAD with changes' '
	cd repo &&
	c2=$(cat c2) &&
	c3=$(cat c3) &&
	test_must_fail git diff --quiet "$c2" "$c3"
'

test_expect_success 'diff --exit-code returns 0 for identical commits' '
	cd repo &&
	c3=$(cat c3) &&
	git diff --exit-code "$c3" "$c3"
'

test_expect_success 'diff --exit-code returns 1 for different commits' '
	cd repo &&
	c1=$(cat c1) &&
	c2=$(cat c2) &&
	test_must_fail git diff --exit-code "$c1" "$c2"
'

test_expect_success 'diff --quiet suppresses output even with differences' '
	cd repo &&
	c1=$(cat c1) &&
	c2=$(cat c2) &&
	git diff --quiet "$c1" "$c2" >out 2>&1 || true &&
	test_must_be_empty out
'

test_expect_success 'diff --exit-code with pathspec: no match means 0' '
	cd repo &&
	c1=$(cat c1) &&
	c2=$(cat c2) &&
	git diff --exit-code "$c1" "$c2" -- nonexistent
'

test_expect_success 'diff --exit-code with pathspec: match means 1' '
	cd repo &&
	c1=$(cat c1) &&
	c2=$(cat c2) &&
	test_must_fail git diff --exit-code "$c1" "$c2" -- a
'

test_expect_success 'diff-index --cached --exit-code after adding more files' '
	cd repo &&
	echo 3 >c &&
	git update-index --add c &&
	c4=$(make_commit fourth "$(cat c3)") &&
	printf "%s\n" "$c4" >c4 &&
	c3=$(cat c3) &&
	test_must_fail git diff-index --exit-code --cached "$c3"
'

# ---------------------------------------------------------------------------
# Additional diff-files format and clean-state tests
# ---------------------------------------------------------------------------

test_expect_success 'diff-files shows no output when clean' '
	cd repo &&
	git diff-files >out &&
	test_must_be_empty out
'

test_expect_success 'diff-files --name-only is empty when clean' '
	cd repo &&
	git diff-files --name-only >out &&
	test_must_be_empty out
'

test_expect_success 'diff-files --name-status shows M for modified file' '
	cd repo &&
	echo extra >>a &&
	git diff-files --name-status >out &&
	grep "^M.*a" out
'

test_expect_success 'diff --name-only same commit shows no output' '
	cd repo &&
	c4=$(cat c4) &&
	git diff --name-only "$c4" "$c4" >out &&
	test_must_be_empty out
'

# ---------------------------------------------------------------------------
# Additional return value tests
# ---------------------------------------------------------------------------

test_expect_success 'diff --quiet same commit returns 0' '
	cd repo &&
	c4=$(cat c4) &&
	git diff --quiet "$c4" "$c4"
'

test_expect_success 'diff --numstat same commit shows no output' '
	cd repo &&
	c4=$(cat c4) &&
	git diff --numstat "$c4" "$c4" >out &&
	test_must_be_empty out
'

test_expect_success 'diff-files --quiet returns 0 when clean' '
	cd repo &&
	git update-index a &&
	git diff-files --quiet
'

test_expect_success 'diff-files --name-only is empty when clean' '
	cd repo &&
	git diff-files --name-only >out &&
	test_must_be_empty out
'

test_expect_success 'setup clean state for retval tests' '
	cd repo &&
	git update-index a b c &&
	c5=$(make_commit fifth "$(cat c4)") &&
	printf "%s\n" "$c5" >c5
'

test_expect_success 'diff --exit-code returns 0 for identical index vs HEAD' '
	cd repo &&
	c5=$(cat c5) &&
	git diff --exit-code --cached "$c5"
'

test_expect_success 'diff-index --quiet --cached returns 0 when same' '
	cd repo &&
	c5=$(cat c5) &&
	git diff-index --quiet --cached "$c5"
'

test_expect_success 'diff --stat same commit is empty' '
	cd repo &&
	c5=$(cat c5) &&
	git diff --stat "$c5" "$c5" >out &&
	test_must_be_empty out
'

test_expect_success 'diff-files --exit-code succeeds after re-staging' '
	cd repo &&
	git diff-files --exit-code
'

test_expect_success 'diff --quiet between parent and child returns 1' '
	cd repo &&
	c3=$(cat c3) && c4=$(cat c4) &&
	test_must_fail git diff --quiet "$c3" "$c4"
'

test_expect_success 'diff --exit-code between parent and child returns 1' '
	cd repo &&
	c3=$(cat c3) && c4=$(cat c4) &&
	test_must_fail git diff --exit-code "$c3" "$c4"
'

test_done
