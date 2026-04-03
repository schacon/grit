#!/bin/sh
#
# Copyright (c) 2006 Catalin Marinas
#

test_description='git apply trying to add an ending line.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'apply adds a line at the end' '
	cat >test-patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1,2 +1,3 @@
	 a
	 b
	+c
	EOF

	printf "a\nb\n" >file &&
	git apply test-patch &&
	printf "a\nb\nc\n" >expected &&
	test_cmp expected file
'

test_expect_success 'apply adds a line at the beginning' '
	cat >test-patch2 <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1,2 +1,3 @@
	+a
	 b
	 c
	EOF

	printf "b\nc\n" >file &&
	git apply test-patch2 &&
	printf "a\nb\nc\n" >expected &&
	test_cmp expected file
'

test_expect_success 'apply --check on applicable patch' '
	printf "a\nb\n" >file &&
	git apply --check test-patch
'

test_expect_success 'apply --stat shows diffstat' '
	git apply --stat test-patch >stat_output &&
	grep "file" stat_output
'

test_done
