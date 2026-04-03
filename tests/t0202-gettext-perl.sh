#!/bin/sh

test_description='Perl gettext interface (Git::I18N)'

. ./test-lib.sh

# Perl gettext tests require lib-gettext.sh and Perl modules
# which are not applicable to grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'Perl gettext tests (requires Perl and lib-gettext)' '
	false
'

test_done
