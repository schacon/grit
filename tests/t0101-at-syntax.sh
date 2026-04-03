#!/bin/sh

test_description='various @{whatever} syntax tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit one &&
	test_commit two
'

test_expect_success '@{0} shows current' '
	echo two >expect &&
	git log --max-count=1 --format=%s "@{0}" >actual &&
	test_cmp expect actual
'

test_expect_success '@{1} shows old' '
	echo one >expect &&
	git log --max-count=1 --format=%s "@{1}" >actual &&
	test_cmp expect actual
'

test_expect_success '@{now} shows current' '
	echo two >expect &&
	git log --max-count=1 --format=%s "@{now}" >actual &&
	test_cmp expect actual
'

test_expect_success 'notice misspelled upstream' '
	test_must_fail git log --max-count=1 --format=%s @{usptream}
'

test_expect_success 'complain about total nonsense' '
	test_must_fail git log --max-count=1 --format=%s @{utter.bogosity}
'

test_done
