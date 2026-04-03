#!/bin/sh

test_description='pack-objects object selection'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup repo' '
	git init &&
	git config init.defaultBranch main &&
	test_commit initial &&
	for i in 1 2 3
	do
		mkdir -p f$i/f$i &&
		echo $i >f$i/f$i/data.txt || return 1
	done &&
	git add . &&
	git commit -m "Initialized trees" &&
	for i in 1 2 3
	do
		git checkout -b topic$i main &&
		echo change-$i >f$i/f$i/data.txt &&
		git commit -a -m "Changed f$i/f$i/data.txt" || return 1
	done
'

test_expect_success 'pack-objects --revs --stdout produces valid pack' '
	echo topic1 >packinput.txt &&
	git pack-objects --stdout --revs <packinput.txt >test.pack &&
	git index-pack --stdin <test.pack
'

test_expect_success 'pack-objects --all produces valid pack' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'verify-pack -v shows all object types' '
	git verify-pack -v testpack-*.pack >output &&
	test_grep "commit" output &&
	test_grep "tree" output &&
	test_grep "blob" output
'

test_done
