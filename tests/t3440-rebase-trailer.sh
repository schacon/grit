#!/bin/sh

test_description='git rebase basic trailer and signoff tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup repo with a small history' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m "base commit" &&
	git tag base &&

	echo first >file &&
	git add file &&
	test_tick &&
	git commit -m "first change" &&
	git tag first &&

	echo second >file &&
	git add file &&
	test_tick &&
	git commit -m "second change" &&
	git tag second
'

test_expect_success 'rebase preserves commit messages' '
	git checkout -b msg-test first &&
	echo extra >extra &&
	git add extra &&
	test_tick &&
	git commit -m "extra commit" &&
	git rebase second &&
	git log --format=%s --max-count=1 >actual &&
	echo "extra commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'rebase with --onto preserves messages' '
	git checkout -b onto-test base &&
	echo onto-extra >onto-extra &&
	git add onto-extra &&
	test_tick &&
	git commit -m "onto extra" &&
	git rebase --onto second base &&
	git log --format=%s --max-count=1 >actual &&
	echo "onto extra" >expect &&
	test_cmp expect actual
'

test_done
