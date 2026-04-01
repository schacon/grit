#!/bin/sh
# Ported subset from git/t/t4013-diff-various.sh for diff-index -m behavior.

test_description='diff-index default vs -m for missing worktree files'

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

test_expect_success 'setup repository with one tracked file' '
	git init repo &&
	cd repo &&
	printf "one\n" >file1 &&
	git update-index --add file1 &&
	commit1=$(make_commit initial) &&
	test -n "$commit1" &&
	printf "%s\n" "$commit1" >commit1
'

test_expect_success 'diff-index reports removed file by default' '
	cd repo &&
	commit1=$(cat commit1) &&
	rm -f file1 &&
	git diff-index "$commit1" >without_m &&
	lines=$(wc -l <without_m) &&
	test "$lines" -eq 1 &&
	grep " D	file1$" without_m
'

test_expect_success 'diff-index -m hides missing working-tree file' '
	cd repo &&
	commit1=$(cat commit1) &&
	git diff-index -m "$commit1" >with_m &&
	lines=$(wc -l <with_m) &&
	test "$lines" -eq 0
'

test_expect_success '--cached mode ignores missing working-tree file' '
	cd repo &&
	commit1=$(cat commit1) &&
	git diff-index --cached --exit-code "$commit1"
'

test_done
