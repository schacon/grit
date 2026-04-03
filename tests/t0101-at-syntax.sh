#!/bin/sh

test_description='various @{whatever} syntax tests'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	test_commit one &&
	test_commit two
'

test_expect_success 'notice misspelled upstream' '
	test_must_fail git log --max-count=1 --format=%s @{usptream}
'

test_expect_success 'complain about total nonsense' '
	test_must_fail git log --max-count=1 --format=%s @{utter.bogosity}
'

test_done
