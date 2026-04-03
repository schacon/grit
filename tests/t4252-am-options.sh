#!/bin/sh

test_description='git am with options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "initial content" >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'am applies a simple mbox patch' '
	cat >patch.mbox <<-\EOF &&
	From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001
	From: Patch Author <patch@example.com>
	Date: Thu, 7 Apr 2005 15:13:13 -0700
	Subject: [PATCH] Modify file

	This modifies the file.
	---
	 file | 2 +-
	 1 file changed, 1 insertion(+), 1 deletion(-)

	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1 +1 @@
	-initial content
	+modified content
	--
	2.0.0

	EOF
	git am patch.mbox &&
	echo "modified content" >expect &&
	test_cmp expect file
'

test_expect_success 'am preserves author' '
	git log -n 1 --format="%an" >actual &&
	echo "Patch Author" >expect &&
	test_cmp expect actual
'

test_expect_success 'am preserves commit message' '
	git log -n 1 --format="%s" >actual &&
	echo "Modify file" >expect &&
	test_cmp expect actual
'

test_expect_success 'am --dry-run does not modify' '
	cat >patch2.mbox <<-\EOF &&
	From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001
	From: Another Author <another@example.com>
	Date: Thu, 7 Apr 2005 15:14:13 -0700
	Subject: [PATCH] Another change

	Another modification.
	---
	 file | 2 +-
	 1 file changed, 1 insertion(+), 1 deletion(-)

	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1 +1 @@
	-modified content
	+another content
	--
	2.0.0

	EOF
	head_before=$(git rev-parse HEAD) &&
	git am --dry-run patch2.mbox &&
	head_after=$(git rev-parse HEAD) &&
	test "$head_before" = "$head_after"
'

test_expect_success 'am applies second patch' '
	git am patch2.mbox &&
	echo "another content" >expect &&
	test_cmp expect file &&
	git log -n 1 --format="%an" >actual &&
	echo "Another Author" >expect &&
	test_cmp expect actual
'

test_expect_success 'am from stdin' '
	cat >patch3.mbox <<-\EOF &&
	From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001
	From: Stdin Author <stdin@example.com>
	Date: Thu, 7 Apr 2005 15:15:13 -0700
	Subject: [PATCH] Stdin change

	Change from stdin.
	---
	 file | 2 +-
	 1 file changed, 1 insertion(+), 1 deletion(-)

	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1 +1 @@
	-another content
	+stdin content
	--
	2.0.0

	EOF
	git am <patch3.mbox &&
	echo "stdin content" >expect &&
	test_cmp expect file
'

test_done
