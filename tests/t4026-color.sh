#!/bin/sh

test_description='Test diff/status color handling'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'unknown color slots are ignored (diff)' '
	git config color.diff.nosuchslotwilleverbedefined white &&
	echo hi >file &&
	git add file &&
	git diff --cached --color
'

test_expect_success 'unknown color slots are ignored (branch)' '
	git commit -m init &&
	git config color.branch.nosuchslotwilleverbedefined white &&
	git branch -a
'

test_expect_success 'unknown color slots are ignored (status)' '
	git config color.status.nosuchslotwilleverbedefined white &&
	{ git status; ret=$?; } &&
	case $ret in 0|1) : ok ;; *) false ;; esac
'

test_done
