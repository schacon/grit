#!/bin/sh

test_description='Test git stash show configuration.'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo 1 >file.t &&
	git add file.t &&
	test_tick &&
	git commit -m "initial"
'

test_expect_success 'stash show with default config shows --stat' '
	echo 2 >file.t &&
	git stash &&
	git stash show >actual &&
	grep "file.t" actual &&
	grep "|" actual
'

test_expect_success 'stash show --stat' '
	git stash show --stat >actual &&
	grep "file.t" actual &&
	grep "|" actual
'

test_expect_success 'stash show -p' '
	git stash show -p >actual &&
	grep "^diff" actual
'

test_expect_success 'stash show with stash ref' '
	git stash show stash@{0} >actual &&
	grep "file.t" actual
'

test_expect_success 'stash pop restores changes' '
	git stash pop &&
	echo 2 >expect &&
	test_cmp expect file.t
'

test_expect_success 'stash show -p shows diff output' '
	echo 3 >file.t &&
	git stash &&
	git stash show -p >actual &&
	grep "^diff" actual &&
	git stash pop
'

test_done
