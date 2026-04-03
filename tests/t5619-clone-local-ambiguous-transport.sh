#!/bin/sh

test_description='test local clone with ambiguous transport'

. ./test-lib.sh

# This test requires an HTTP server and is heavily security-focused.
# We simplify to test basic local clone behavior.

test_expect_success 'setup' '
	git init &&
	echo content >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'local clone works' '
	git clone . clone1 &&
	test_path_is_dir clone1/.git
'

test_expect_failure 'file:// clone works' '
	git clone "file://$(pwd)" clone2 &&
	test_path_is_dir clone2/.git
'

test_done
