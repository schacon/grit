#!/bin/sh
# Ported from git/t/t5900-repo-selection.sh
# Tests for selecting remote repo in ambiguous cases

test_description='selecting remote repo in ambiguous cases'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

reset_repos () {
	rm -rf foo foo.git fetch clone
}

make_tree () {
	git init "$1" &&
	(cd "$1" &&
	 echo "$1" >file &&
	 git add file &&
	 test_tick &&
	 git commit -m "$1"
	)
}

make_bare () {
	git init --bare "$1" &&
	(cd "$1" &&
	 tree=$(git hash-object -w -t tree /dev/null) &&
	 commit=$(echo "$1" | git commit-tree $tree) &&
	 git update-ref HEAD $commit
	)
}

test_expect_success 'find .git dir in worktree' '
	reset_repos &&
	make_tree foo &&
	git clone foo clone &&
	(cd clone && git log -1 --format=%s HEAD) >actual &&
	echo foo >expect &&
	test_cmp expect actual
'

test_expect_failure 'automagically add .git suffix' '
	reset_repos &&
	make_bare foo.git &&
	git clone foo clone &&
	test -d clone/.git
'

test_expect_failure 'automagically add .git suffix to worktree' '
	reset_repos &&
	make_tree foo.git &&
	git clone foo clone &&
	test -d clone/.git
'

test_expect_success 'prefer worktree foo over bare foo.git' '
	reset_repos &&
	make_tree foo &&
	make_bare foo.git &&
	git clone foo clone &&
	(cd clone && git log -1 --format=%s HEAD) >actual &&
	echo foo >expect &&
	test_cmp expect actual
'

test_expect_failure 'we are not fooled by non-git foo directory' '
	reset_repos &&
	make_bare foo.git &&
	mkdir foo &&
	git clone foo clone &&
	test -d clone/.git
'

test_done
