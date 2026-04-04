#!/bin/sh

test_description='Gettext support for Git (grit passthrough)'

. ./test-lib.sh

# In grit, gettext is a simple passthrough — no translation.
# We verify the sh-i18n helper works correctly.

test_expect_success 'setup' '
	git init
'

test_expect_success 'gettext passthrough returns input unchanged' '
	echo "hello world" >expect &&
	git sh-i18n "hello world" >actual &&
	test_cmp expect actual
'

test_done
