#!/bin/sh
#
# Ported from git/t/t1311-config-optional.sh
# Tests config get --default behavior.
# Note: :(optional) paths and --show-scope not implemented in grit.

test_description='config get with default values'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success 'config get returns default when key missing' '
	echo fallback >expect &&
	git config get --default=fallback nonexistent.key >actual &&
	test_cmp expect actual
'

test_expect_success 'config get returns actual value when key present' '
	git config set test.key "realval" &&
	echo realval >expect &&
	git config get --default=fallback test.key >actual &&
	test_cmp expect actual
'

test_done
