#!/bin/sh

test_description='git rebase with multiple commits'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m "base" &&
	git tag base &&

	git checkout -b topic &&
	echo one >>file &&
	git add file &&
	test_tick &&
	git commit -m "first on topic" &&

	echo two >>file &&
	git add file &&
	test_tick &&
	git commit -m "second on topic" &&

	echo three >>file &&
	git add file &&
	test_tick &&
	git commit -m "third on topic" &&

	git checkout main &&
	echo main-change >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m "main advance"
'

test_expect_success 'rebase multiple commits onto main' '
	git checkout topic &&
	git rebase main &&
	test -f file &&
	test -f file2
'

test_expect_success 'all commits rebased' '
	git rev-parse main >expect &&
	git rev-parse HEAD~3 >actual &&
	test_cmp expect actual
'

test_expect_success 'commit messages preserved' '
	git log --format=%s -n3 >actual &&
	cat >expect <<-\EOF &&
	third on topic
	second on topic
	first on topic
	EOF
	test_cmp expect actual
'

test_expect_success 'file content is correct after rebase' '
	cat >expect <<-\EOF &&
	base
	one
	two
	three
	EOF
	test_cmp expect file
'

test_done
