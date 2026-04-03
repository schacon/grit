#!/bin/sh

test_description='Gettext Shell fallbacks'

. ./test-lib.sh

# Gettext fallback tests require lib-gettext.sh
# which is not applicable to grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'gettext fallback tests (requires gettext infrastructure)' '
	test -n "$GIT_INTERNAL_GETTEXT_SH_SCHEME"
'

test_done
