#!/bin/sh

test_description='git am with DOS line endings'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "initial" >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'am applies patch with unix line endings' '
	cat >unix-patch.mbox <<-\EOF &&
	From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001
	From: Test Author <author@example.com>
	Date: Thu, 7 Apr 2005 15:13:13 -0700
	Subject: [PATCH] Unix line endings

	This patch has unix line endings.
	---
	 file | 2 +-
	 1 file changed, 1 insertion(+), 1 deletion(-)

	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1 +1 @@
	-initial
	+changed
	--
	2.0.0

	EOF
	git am unix-patch.mbox &&
	echo "changed" >expect &&
	test_cmp expect file
'

test_expect_success 'am preserves commit message' '
	git log -n 1 --format="%s" >actual &&
	echo "Unix line endings" >expect &&
	test_cmp expect actual
'

test_expect_success 'am preserves author info' '
	git log -n 1 --format="%an" >actual &&
	echo "Test Author" >expect &&
	test_cmp expect actual
'

test_expect_success 'am --quiet suppresses output' '
	cat >quiet-patch.mbox <<-\EOF &&
	From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001
	From: Another Author <another@example.com>
	Date: Thu, 7 Apr 2005 15:14:13 -0700
	Subject: [PATCH] Quiet patch

	Quiet commit message.
	---
	 file | 2 +-
	 1 file changed, 1 insertion(+), 1 deletion(-)

	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1 +1 @@
	-changed
	+quiet change
	--
	2.0.0

	EOF
	git am -q quiet-patch.mbox &&
	echo "quiet change" >expect &&
	test_cmp expect file
'

test_done
