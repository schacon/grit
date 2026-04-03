#!/bin/sh

test_description='Intent to add'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'intent to add' '
	test_commit 1 &&
	echo hello >file &&
	echo hello >elif &&
	git add -N file &&
	git add elif
'

test_expect_success 'check result of "add -N"' '
	git ls-files -s file >actual &&
	grep "100644" actual &&
	grep "file" actual
'

test_expect_success 'intent to add is just an ordinary empty blob after add -u' '
	git add -u &&
	git ls-files -s file >actual &&
	git ls-files -s elif | sed -e "s/elif/file/" >expect &&
	test_cmp expect actual
'

test_done
