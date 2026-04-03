#!/bin/sh

test_description='Gettext reencoding of *.po/*.mo files'

. ./test-lib.sh

# Gettext reencoding tests require locale infrastructure
# not applicable to grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'gettext reencoding tests (requires locale infrastructure)' '
	false
'

test_done
