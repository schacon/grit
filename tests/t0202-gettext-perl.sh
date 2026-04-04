#!/bin/sh

test_description='Gettext Perl interface (not applicable to grit)'

. ./test-lib.sh

# Grit does not use Perl for gettext. Verify basic i18n plumbing
# by checking that grit produces expected English output.

test_expect_success 'setup' '
	git init
'

test_expect_success 'grit produces English messages without Perl gettext' '
	git status >out 2>&1 &&
	grep -i -e "branch" -e "nothing" -e "commit" out
'

test_done
