#!/bin/sh

test_description='Gettext interface (grit passthrough)'

. ./test-lib.sh

# Perl gettext is not applicable to grit.
# Verify the sh-i18n passthrough handles multiple words.

test_expect_success 'setup' '
	git init
'

test_expect_success 'gettext passthrough handles multiple arguments' '
	echo "multiple words here" >expect &&
	git sh-i18n multiple words here >actual &&
	test_cmp expect actual
'

test_done
