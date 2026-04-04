#!/bin/sh

test_description='Gettext reencoding (grit passthrough)'

. ./test-lib.sh

# Gettext reencoding is not applicable to grit.
# Verify passthrough handles special characters.

test_expect_success 'setup' '
	git init
'

test_expect_success 'gettext passthrough handles special characters' '
	echo "café résumé naïve" >expect &&
	git sh-i18n "café résumé naïve" >actual &&
	test_cmp expect actual
'

test_done
