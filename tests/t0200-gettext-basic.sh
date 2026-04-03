#!/bin/sh

test_description='Gettext support for Git'

. ./test-lib.sh

# Gettext tests require lib-gettext.sh and locale infrastructure
# which is not applicable to grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'gettext basic tests (requires locale infrastructure)' '
	test -n "$GIT_INTERNAL_GETTEXT_SH_SCHEME"
'

test_done
