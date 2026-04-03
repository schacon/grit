#!/bin/sh

test_description='check problems with relative GIT_DIR

This test creates a working tree state with a file and subdir
and tests commits from different directories.'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'commit from top level' '
	echo initial >top &&
	git add top &&
	git commit -m initial
'

test_expect_success 'commit from subdir' '
	mkdir -p subdir &&
	echo changed >top &&
	git add top &&
	git commit -m "changed top" &&
	(
		cd subdir &&
		echo sub >sub &&
		git add sub &&
		git commit -m "add sub"
	)
'

test_expect_success 'GIT_DIR from env works' '
	mkdir -p otherdir &&
	(
		cd otherdir &&
		GIT_DIR="$(pwd)/../.git" &&
		export GIT_DIR &&
		git rev-parse HEAD
	)
'

test_done
