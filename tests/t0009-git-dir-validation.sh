#!/bin/sh

test_description='setup: validation of .git file/directory types'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init parent &&
	(cd parent && git config user.name "Test" && git config user.email "t@t" && test_commit root-commit)
'

test_expect_success 'setup: .git with garbage content is rejected' '
	test_when_finished "rm -rf parent/garbage-trap" &&
	mkdir -p parent/garbage-trap &&
	(
		cd parent/garbage-trap &&
		echo "garbage" >.git &&
		test_must_fail git rev-parse --git-dir 2>stderr
	)
'

test_done
