#!/bin/sh

test_description='magic pathspec tests using git-add'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	mkdir sub anothersub &&
	: >sub/foo &&
	: >anothersub/foo
'

test_expect_success 'add :/non-existent fails' '
	(cd sub && test_must_fail git add -n :/non-existent)
'

test_expect_success 'add files from subdirectory' '
	(cd sub && git add foo) &&
	git ls-files >actual &&
	grep "sub/foo" actual
'

test_done
