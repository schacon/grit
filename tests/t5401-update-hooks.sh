#!/bin/sh
# Ported from git/t/t5401-update-hooks.sh
# Simplified: tests basic send-pack functionality

test_description='Test send-pack with bare repositories'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo This is a test. >a &&
	git update-index --add a &&
	tree0=$(git write-tree) &&
	commit0=$(echo setup | git commit-tree $tree0) &&
	echo We hope it works. >a &&
	git update-index a &&
	tree1=$(git write-tree) &&
	commit1=$(echo modify | git commit-tree $tree1 -p $commit0) &&
	git update-ref refs/heads/main $commit0 &&
	echo "$commit0" >saved-commit0 &&
	echo "$commit1" >saved-commit1 &&
	git clone --bare ./. victim.git &&
	git update-ref refs/heads/main $commit1
'

test_expect_success 'send-pack updates bare repo' '
	commit1=$(cat saved-commit1) &&
	git send-pack ./victim.git main &&
	actual=$(git --git-dir=victim.git rev-parse main) &&
	test "$actual" = "$commit1"
'

test_expect_success 'send-pack with multiple refs' '
	commit0=$(cat saved-commit0) &&
	git branch side $commit0 &&
	git send-pack ./victim.git main side &&
	actual=$(git --git-dir=victim.git rev-parse side) &&
	test "$actual" = "$commit0"
'

test_done
