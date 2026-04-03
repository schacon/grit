#!/bin/sh
#
# Ported subset from git/t/t4138-apply-ws-expansion.sh

test_description='git apply with patches that add and remove lines'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo
'

test_expect_success 'apply patch inserting lines in the middle' '
	cd repo &&
	test_write_lines 1 2 3 4 5 6 >test-file &&
	git add test-file &&
	git commit -m "initial" &&
	cat >insert.patch <<-\EOF &&
	diff --git a/test-file b/test-file
	--- a/test-file
	+++ b/test-file
	@@ -1,6 +1,9 @@
	 1
	 2
	 3
	+a
	+b
	+c
	 4
	 5
	 6
	EOF
	git apply insert.patch &&
	test_write_lines 1 2 3 a b c 4 5 6 >expect &&
	test_cmp expect test-file
'

test_expect_success 'apply patch removing lines from the middle' '
	cd repo &&
	test_write_lines 1 2 3 a b c 4 5 6 >test-file &&
	cat >remove.patch <<-\EOF &&
	diff --git a/test-file b/test-file
	--- a/test-file
	+++ b/test-file
	@@ -1,9 +1,6 @@
	 1
	 2
	 3
	-a
	-b
	-c
	 4
	 5
	 6
	EOF
	git apply remove.patch &&
	test_write_lines 1 2 3 4 5 6 >expect &&
	test_cmp expect test-file
'

test_expect_success 'apply patch with mixed additions and deletions' '
	cd repo &&
	test_write_lines 1 2 3 4 5 6 >test-file &&
	cat >mixed.patch <<-\EOF &&
	diff --git a/test-file b/test-file
	--- a/test-file
	+++ b/test-file
	@@ -1,6 +1,6 @@
	 1
	-2
	+two
	 3
	 4
	-5
	+five
	 6
	EOF
	git apply mixed.patch &&
	test_write_lines 1 two 3 4 five 6 >expect &&
	test_cmp expect test-file
'

test_done
