#!/bin/sh
# Adapted from git/t/t7008-filter-branch-null-sha1.sh
# Tests basic repo operations that filter-branch tests rely on

test_description='operations tested by filter-branch null sha1 test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: base commits' '
	git init fb-null &&
	cd fb-null &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo one >one.t &&
	git add one.t &&
	git commit -m "one" &&
	git tag one &&

	echo two >two.t &&
	git add two.t &&
	git commit -m "two" &&
	git tag two &&

	echo three >three.t &&
	git add three.t &&
	git commit -m "three" &&
	git tag three
'

test_expect_success 'ls-tree works' '
	cd fb-null &&
	git ls-tree HEAD >output &&
	test_grep "one.t" output &&
	test_grep "two.t" output &&
	test_grep "three.t" output
'

test_expect_success 'mktree works' '
	cd fb-null &&
	git ls-tree HEAD >tree-listing &&
	tree=$(git mktree <tree-listing) &&
	test -n "$tree"
'

test_expect_success 'commit-tree works' '
	cd fb-null &&
	tree=$(git rev-parse HEAD^{tree}) &&
	commit=$(echo "test message" | git commit-tree "$tree") &&
	test -n "$commit" &&
	git cat-file -t "$commit" >type &&
	test_grep "commit" type
'

test_done
