#!/bin/sh

test_description='Gettext fallback support'

. ./test-lib.sh

# Grit uses passthrough gettext — messages are always in English.
# Test that basic error messages appear in English.

test_expect_success 'setup' '
	git init
'

test_expect_success 'gettext fallback: error messages are in English' '
	test_must_fail git checkout nonexistent 2>err &&
	cat err &&
	grep -i -e "error" -e "not" -e "pathspec" -e "did not match" err
'

test_done
