#!/bin/sh

test_description='Test cloning a repository larger than 2 gigabyte'
. ./test-lib.sh

if ! test_bool_env GIT_TEST_CLONE_2GB false
then
	skip_all='expensive 2GB clone test; enable with GIT_TEST_CLONE_2GB=true'
	test_done
fi

test_expect_success 'setup' '
	git init &&
	git config pack.compression 0 &&
	git config pack.depth 0 &&
	# Create a moderately large repo
	i=1 &&
	while test $i -le 10
	do
		dd if=/dev/zero bs=1024 count=1024 2>/dev/null | tr "\\0" "x" >largefile_$i &&
		git add largefile_$i &&
		git commit -m "large file $i" || exit
		i=$(($i + 1))
	done
'

test_expect_success 'clone - bare' '
	git clone --bare --no-hardlinks . clone-bare
'

test_expect_success 'clone - with worktree' '
	git clone . clone-wt
'

test_done
