#!/bin/sh
#
# Copyright (c) 2006 Eric Wong
#

test_description='git apply should not get confused with type changes.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository and commits' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "hello world" >foo &&
	echo "hi planet" >bar &&
	git add foo bar &&
	git commit -m initial &&
	git branch initial
'

test_expect_success 'apply modification patch' '
	echo "hello world" >foo &&
	echo "modified content" >foo.new &&
	cat >mod-patch <<-\EOF &&
	diff --git a/foo b/foo
	--- a/foo
	+++ b/foo
	@@ -1 +1 @@
	-hello world
	+modified content
	EOF
	git apply mod-patch &&
	echo "modified content" >expect &&
	test_cmp expect foo
'

test_expect_success 'apply deletion patch' '
	echo "hello world" >foo &&
	cat >del-patch <<-\EOF &&
	diff --git a/foo b/foo
	deleted file mode 100644
	--- a/foo
	+++ /dev/null
	@@ -1 +0,0 @@
	-hello world
	EOF
	git apply del-patch &&
	test_path_is_missing foo
'

test_expect_success 'apply creation patch' '
	test_path_is_missing newfile &&
	cat >create-patch <<-\EOF &&
	diff --git a/newfile b/newfile
	new file mode 100644
	--- /dev/null
	+++ b/newfile
	@@ -0,0 +1 @@
	+brand new content
	EOF
	git apply create-patch &&
	echo "brand new content" >expect &&
	test_cmp expect newfile
'

test_expect_success 'apply --reverse of creation removes file' '
	echo "brand new content" >newfile &&
	git apply -R create-patch &&
	test_path_is_missing newfile
'

test_expect_success 'apply multi-hunk patch' '
	echo "hello world" >foo &&
	test_write_lines 1 2 3 4 5 6 7 8 9 10 >multi &&
	cat >multi-patch <<-\EOF &&
	diff --git a/multi b/multi
	--- a/multi
	+++ b/multi
	@@ -1,4 +1,4 @@
	 1
	-2
	+TWO
	 3
	 4
	@@ -7,4 +7,4 @@
	 7
	 8
	-9
	+NINE
	 10
	EOF
	git apply multi-patch &&
	grep TWO multi &&
	grep NINE multi
'

test_done
