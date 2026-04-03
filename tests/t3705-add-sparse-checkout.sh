#!/bin/sh

test_description='add with sparse checkout'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo a >a &&
	echo b >b &&
	echo c >c &&
	mkdir sub &&
	echo d >sub/d &&
	git add -A &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'sparse-checkout init works' '
	git sparse-checkout init
'

test_expect_success 'sparse-checkout set limits working tree' '
	git sparse-checkout set a &&
	test_path_is_file a
'

test_expect_success 'sparse-checkout list shows patterns' '
	git sparse-checkout list >actual &&
	grep "a" actual
'

test_expect_success 'sparse-checkout disable restores all files' '
	git sparse-checkout disable &&
	test_path_is_file a &&
	test_path_is_file b &&
	test_path_is_file c
'

test_done
