#!/bin/sh

test_description='anchored diff algorithm

Most upstream tests use --no-index which grit does not support yet.
The last test uses git show --anchored which also needs plumbing support.'

. ./test-lib.sh

test_expect_failure '--anchored (not implemented: needs --no-index)' '
	printf "a\nb\nc\n" >pre &&
	printf "c\na\nb\n" >post &&
	test_expect_code 1 git diff --no-index pre post >diff &&
	test -s diff &&
	grep "^+c" diff &&
	test_expect_code 1 git diff --no-index --anchored=c pre post >diff &&
	test -s diff &&
	grep "^+a" diff
'

test_expect_failure '--anchored multiple (not implemented: needs --no-index)' '
	printf "a\nb\nc\nd\ne\nf\n" >pre &&
	printf "c\na\nb\nf\nd\ne\n" >post &&
	test_expect_code 1 git diff --no-index --anchored=c pre post >diff &&
	test -s diff &&
	grep "^+a" diff &&
	grep "^+f" diff &&
	test_expect_code 1 git diff --no-index --anchored=c --anchored=f pre post >diff &&
	test -s diff &&
	grep "^+a" diff &&
	grep "^+d" diff
'

test_expect_success '--anchored with nonexistent line has no effect (not implemented)' '
	printf "a\nb\nc\n" >pre &&
	printf "c\na\nb\n" >post &&
	test_expect_code 1 git diff --no-index --anchored=x pre post >diff &&
	test -s diff &&
	grep "^+c" diff
'

test_expect_success '--anchored with non-unique line has no effect (not implemented)' '
	printf "a\nb\nc\nd\ne\nc\n" >pre &&
	printf "c\na\nb\nc\nd\ne\n" >post &&
	test_expect_code 1 git diff --no-index --anchored=c pre post >diff &&
	test -s diff &&
	grep "^+c" diff
'

test_expect_failure 'diff still produced with impossible multiple --anchored (not implemented)' '
	printf "a\nb\nc\n" >pre &&
	printf "c\na\nb\n" >post &&
	test_expect_code 1 git diff --no-index --anchored=a --anchored=c pre post >diff &&
	test -s diff &&
	mv post expected_post &&
	git apply diff &&
	diff expected_post post
'

test_expect_failure 'later algorithm arguments override earlier ones (not implemented)' '
	printf "a\nb\nc\n" >pre &&
	printf "c\na\nb\n" >post &&
	test_expect_code 1 git diff --no-index --patience --anchored=c pre post >diff &&
	test -s diff &&
	grep "^+a" diff
'

test_expect_failure '--anchored works with other commands like git show (not implemented)' '
	printf "a\nb\nc\n" >file &&
	git add file &&
	git commit -m foo &&
	printf "c\na\nb\n" >file &&
	git add file &&
	git commit -m foo &&
	git show --patience --anchored=c >diff &&
	grep "^+a" diff
'

test_done
