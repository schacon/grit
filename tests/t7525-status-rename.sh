#!/bin/sh

test_description='git status rename detection options'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo 1 >original &&
	git add . &&
	git commit -m "Adding original file." &&
	mv original renamed &&
	git add .
'

test_expect_success 'status shows deleted and new file' '
	git status >actual &&
	test_grep "deleted\|renamed\|new file" actual
'

test_done
