#!/bin/sh

test_description='Test the core.hooksPath configuration variable'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'core.hooksPath can be set and read' '
	git config core.hooksPath /custom/hooks &&
	val=$(git config core.hooksPath) &&
	test "$val" = "/custom/hooks"
'

test_expect_success 'core.hooksPath can be relative' '
	git config core.hooksPath .git/custom-hooks &&
	val=$(git config core.hooksPath) &&
	test "$val" = ".git/custom-hooks"
'

test_done
