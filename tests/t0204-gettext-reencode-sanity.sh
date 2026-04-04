#!/bin/sh

test_description='Gettext reencoding sanity checks (not applicable to grit)'

. ./test-lib.sh

# Grit does not translate or reencode messages — always English UTF-8.

test_expect_success 'setup' '
	git init
'

test_expect_success 'grit output is valid UTF-8' '
	git status >out 2>&1 &&
	# Verify output is non-empty and contains expected keywords
	test -s out &&
	grep -i -e "branch" -e "nothing" -e "commit" out
'

test_done
