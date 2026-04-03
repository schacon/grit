#!/bin/sh

test_description='log/show --expand-tabs'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup commit with tab in message' '
	git commit --allow-empty -m "	tab-indented title" &&
	git commit --allow-empty -m "normal title"
'

test_expect_success 'log shows commit messages' '
	git log --oneline >actual &&
	grep "tab-indented" actual &&
	grep "normal" actual
'

test_done
