#!/bin/sh

test_description='Gettext support for Git'

. ./test-lib.sh

# Grit uses passthrough gettext (no translation, English only).
# We verify that the basic gettext plumbing works.

test_expect_success 'setup' '
	git init
'

test_expect_success 'gettext: grit outputs untranslated messages' '
	# grit does not translate messages — verify a known command
	# produces English output (no locale dependency)
	git status >out 2>&1 &&
	grep -i -e "branch" -e "nothing" -e "commit" out
'

test_done
