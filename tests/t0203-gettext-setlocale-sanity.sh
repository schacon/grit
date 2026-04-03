#!/bin/sh

test_description="The Git C functions aren't broken by setlocale(3)"

. ./test-lib.sh

# Locale tests require lib-gettext.sh infrastructure
# not applicable to grit.

test_expect_success 'setup' '
	git init
'

test_expect_success 'git show works under C locale' '
	git commit --allow-empty -m "test-commit" &&
	git show >out 2>err &&
	grep "test-commit" out
'

test_done
