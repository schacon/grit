#!/bin/sh
# Ported from git/t/t3436-rebase-more-options.sh
# More rebase option tests

test_description='rebase additional options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	test_commit A &&
	test_commit B &&
	test_commit C &&
	git checkout -b feature A &&
	test_commit D &&
	test_commit E &&
	test_commit F
'

test_expect_success 'rebase feature onto master' '
	git checkout feature &&
	git rebase master &&
	git log --format=%s -n5 >actual &&
	test_write_lines F E D C B >expect &&
	test_cmp expect actual
'

test_expect_success 'rebase --onto moves commits to a different base' '
	git checkout -b feature2 A &&
	test_commit G &&
	test_commit H &&
	git rebase --onto B A &&
	parent=$(git rev-parse HEAD~1^) &&
	b_sha=$(git rev-parse B) &&
	test "$parent" = "$b_sha"
'

test_expect_success 'rebase --abort during conflict restores state' '
	git checkout master &&
	git checkout -b base-conflict &&
	echo base >conflict-file &&
	git add conflict-file &&
	git commit -m "add conflict-file" &&
	git checkout -b side-conflict &&
	echo side >conflict-file &&
	git add conflict-file &&
	git commit -m "side change" &&
	git checkout base-conflict &&
	echo main >conflict-file &&
	git add conflict-file &&
	git commit -m "main change" &&

	git checkout side-conflict &&
	old=$(git rev-parse HEAD) &&
	test_must_fail git rebase base-conflict &&
	git rebase --abort &&
	new=$(git rev-parse HEAD) &&
	test "$old" = "$new"
'

test_done
