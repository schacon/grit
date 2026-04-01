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

test_done
