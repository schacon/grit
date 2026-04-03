#!/bin/sh

test_description='git apply with various patch formats'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	test_write_lines a b c d e f g h >file &&
	git add file &&
	git commit -q -m initial
'

test_expect_success 'apply patch with context' '
	test_write_lines a b c d e f g h >file &&
	cat >patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -2,6 +2,7 @@
	 b
	 c
	 d
	+inserted
	 e
	 f
	 g
	EOF
	git apply patch &&
	grep inserted file
'

test_expect_success 'apply patch deleting lines' '
	test_write_lines a b c d e f g h >file &&
	cat >del-patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -2,4 +2,2 @@
	 b
	-c
	-d
	 e
	EOF
	git apply del-patch &&
	! grep c file &&
	! grep d file
'

test_expect_success 'apply patch adding at beginning' '
	test_write_lines a b c >file &&
	cat >begin-patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1,3 +1,4 @@
	+z
	 a
	 b
	 c
	EOF
	git apply begin-patch &&
	head -1 file | grep z
'

test_expect_success 'apply patch adding at end' '
	test_write_lines a b c >file &&
	cat >end-patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1,3 +1,4 @@
	 a
	 b
	 c
	+z
	EOF
	git apply end-patch &&
	tail -1 file | grep z
'

test_expect_success 'apply --check on conflicting patch fails' '
	test_write_lines x y z >file &&
	test_must_fail git apply --check patch
'

test_expect_success 'apply multiple hunks' '
	test_write_lines 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 >file &&
	cat >multi-patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1,5 +1,5 @@
	 1
	-2
	+TWO
	 3
	 4
	 5
	@@ -16,5 +16,5 @@
	 16
	 17
	-18
	+EIGHTEEN
	 19
	 20
	EOF
	git apply multi-patch &&
	grep TWO file &&
	grep EIGHTEEN file
'

test_done
