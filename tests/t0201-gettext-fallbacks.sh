#!/bin/sh

test_description='Gettext Shell fallbacks (grit passthrough)'

. ./test-lib.sh

# In grit, gettext fallback is the passthrough itself.

test_expect_success 'setup' '
	git init
'

test_expect_success 'gettext fallback passes through untranslated text' '
	echo "This is a test message" >expect &&
	git sh-i18n "This is a test message" >actual &&
	test_cmp expect actual
'

test_done
