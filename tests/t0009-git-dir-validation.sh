#!/bin/sh

test_description='setup: validation of .git file/directory types'

. ./test-lib.sh

test_expect_success 'setup: create parent git repository' '
	git init parent &&
	(cd parent && git config user.name "Test" && git config user.email "t@t" && test_commit root-commit)
'

test_expect_failure 'setup: .git as a symlink to a directory is valid' '
	test_have_prereq SYMLINKS || return 0 &&
	test_when_finished "rm -rf parent/link-to-dir" &&
	mkdir -p parent/link-to-dir &&
	(
		cd parent/link-to-dir &&
		git init real-repo &&
		ln -s real-repo/.git .git &&
		git rev-parse --git-dir >actual &&
		echo .git >expect &&
		test_cmp expect actual
	)
'

test_expect_failure 'setup: .git as a FIFO (named pipe) is rejected' '
	test_have_prereq PIPE || return 0 &&
	test_when_finished "rm -rf parent/fifo-trap" &&
	mkdir -p parent/fifo-trap &&
	(
		cd parent/fifo-trap &&
		mkfifo .git &&
		test_must_fail git rev-parse --git-dir 2>stderr
	)
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

test_expect_failure 'setup: .git as an empty directory is ignored' '
	test_when_finished "rm -rf parent/empty-dir" &&
	mkdir -p parent/empty-dir &&
	(
		cd parent/empty-dir &&
		git rev-parse --git-dir >expect &&
		mkdir .git &&
		git rev-parse --git-dir >actual &&
		test_cmp expect actual
	)
'

test_done
