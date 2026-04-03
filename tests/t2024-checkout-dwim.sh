#!/bin/sh

test_description='checkout <branch> basic tests'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	test_commit my_main &&
	git checkout -b feature &&
	test_commit feature_work &&
	git checkout master
'

test_expect_success 'checkout of non-existing branch fails' '
	test_must_fail git checkout xyzzy &&
	test_must_fail git rev-parse --verify refs/heads/xyzzy
'

test_expect_success 'checkout of existing branch succeeds' '
	git checkout feature &&
	echo refs/heads/feature >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_done
