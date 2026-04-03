#!/bin/sh

test_description='applying patch that has broken whitespaces in context'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	>file &&
	git add file &&

	# file-0 is full of content
	printf "%s\n" a bb c d eeee f ggg h >file-0 &&

	# patch-0 creates the file from empty
	cat file-0 >file &&
	git diff >patch-0 &&
	git add file &&

	# file-1 has one line updated
	sed -e "s/d/D/" file-0 >file-1 &&
	cat file-1 >file &&
	git diff >patch-1 &&

	# patch-all is the combined effect
	>file &&
	git add file &&
	cat file-1 >file &&
	git diff >patch-all
'

test_expect_success 'apply patch-0 (create file content)' '
	>file &&
	git add file &&
	git apply patch-0 &&
	test_cmp file-0 file
'

test_expect_success 'apply patch-1 on top of patch-0' '
	>file &&
	git add file &&
	git apply patch-0 &&
	git add file &&
	git apply patch-1 &&
	test_cmp file-1 file
'

test_expect_success 'apply patch-all in one shot' '
	>file &&
	git add file &&
	git apply patch-all &&
	test_cmp file-1 file
'

test_expect_success 'apply --reverse undoes the change' '
	cat file-1 >file &&
	git apply -R patch-1 &&
	test_cmp file-0 file
'

test_done
