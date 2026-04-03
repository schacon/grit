#!/bin/sh
#
# Copyright (c) 2007 Junio C Hamano
#

test_description='git apply and configuration'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo A >file1 &&
	git add file1 &&
	git commit -q -m initial
'

test_expect_success 'create and apply basic patch' '
	echo A >file1 &&
	cat >patch.file <<-\EOF &&
	diff --git a/file1 b/file1
	--- a/file1
	+++ b/file1
	@@ -1 +1 @@
	-A
	+B
	EOF
	git apply patch.file &&
	grep B file1
'

test_expect_success 'apply --check works' '
	echo A >file1 &&
	git apply --check patch.file
'

test_expect_success 'apply --stat shows statistics' '
	git apply --stat patch.file >stat_output &&
	grep "file1" stat_output
'

test_expect_success 'apply --numstat shows machine-readable stats' '
	git apply --numstat patch.file >numstat_output &&
	grep "file1" numstat_output
'

test_expect_success 'apply with -p1 option (default)' '
	echo A >file1 &&
	git apply -p1 patch.file &&
	grep B file1
'

test_expect_success 'apply --summary on extended header' '
	cat >mode-patch <<-\EOF &&
	diff --git a/file1 b/file1
	old mode 100644
	new mode 100755
	EOF
	git apply --summary mode-patch >summary &&
	grep "mode change" summary
'

test_expect_success 'apply --reverse undoes a change' '
	echo B >file1 &&
	git apply -R patch.file &&
	grep A file1
'

test_expect_success 'apply multiple patches sequentially' '
	echo A >file1 &&
	git apply patch.file &&
	grep B file1 &&
	cat >patch2.file <<-\EOF &&
	diff --git a/file1 b/file1
	--- a/file1
	+++ b/file1
	@@ -1 +1 @@
	-B
	+C
	EOF
	git apply patch2.file &&
	grep C file1
'

test_done
