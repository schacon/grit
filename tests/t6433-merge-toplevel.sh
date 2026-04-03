#!/bin/sh
# Ported from git/t/t6433-merge-toplevel.sh
# Tests "git merge" top-level frontend

test_description='"git merge" top-level frontend'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init toplevel &&
	cd toplevel &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo one >one.t &&
	git add one.t &&
	git commit -m "one" &&
	git tag one &&

	git branch left &&
	git branch right &&

	echo two >two.t &&
	git add two.t &&
	git commit -m "two" &&
	git tag two &&

	git checkout left &&
	echo three >three.t &&
	git add three.t &&
	git commit -m "three" &&
	git tag three &&

	git checkout right &&
	echo four >four.t &&
	git add four.t &&
	git commit -m "four" &&
	git tag four &&

	git checkout main
'

test_expect_success 'merge fast-forward' '
	cd toplevel &&
	git reset --hard one &&
	git merge left &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse three)"
'

test_expect_success 'merge no-ff creates merge commit' '
	cd toplevel &&
	git reset --hard two &&
	git merge --no-ff left -m "merge left" &&
	git rev-parse HEAD^1 >actual &&
	git rev-parse two >expect &&
	test_cmp expect actual &&
	git rev-parse HEAD^2 >actual &&
	git rev-parse three >expect &&
	test_cmp expect actual
'

test_expect_success 'merge two branches' '
	cd toplevel &&
	git reset --hard two &&
	git merge left right -m "octopus merge" &&
	test_path_is_file three.t &&
	test_path_is_file four.t
'

test_expect_success 'merge with --squash does not create merge commit' '
	cd toplevel &&
	git reset --hard two &&
	git merge --squash left &&
	git commit -m "squash merge" &&
	# squash merge should not have a second parent
	test_must_fail git rev-parse HEAD^2
'

test_done
