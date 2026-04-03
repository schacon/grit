#!/bin/sh

test_description='checkout with merge scenarios

Tests basic checkout between branches that have diverging content.'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_tick &&
	echo initial >both.txt &&
	git add both.txt &&
	git commit -m initial &&
	git tag initial &&
	git branch topic &&
	echo in_main >both.txt &&
	git add both.txt &&
	test_tick &&
	git commit -m modified_in_main &&
	git checkout topic &&
	echo in_topic >both.txt &&
	git add both.txt &&
	test_tick &&
	git commit -m modified_in_topic
'

test_expect_success 'checkout main from topic switches' '
	git checkout main &&
	echo in_main >expect &&
	test_cmp expect both.txt
'

test_expect_success 'checkout topic from main switches' '
	git checkout topic &&
	echo in_topic >expect &&
	test_cmp expect both.txt
'

test_expect_success 'checkout with tag works' '
	git checkout initial &&
	echo initial >expect &&
	test_cmp expect both.txt &&
	git checkout main
'

test_done
