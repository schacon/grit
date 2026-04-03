#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#

test_description='git mailinfo and git mailsplit test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'mailsplit splits mbox' '
	cat >sample.mbox <<-\MBOX &&
	From nobody Mon Sep 17 00:00:00 2001
	From: Author One <one@example.com>
	Subject: [PATCH 1/2] First patch
	Date: Mon, 17 Sep 2001 00:00:00 +0000

	First commit message body.

	---
	 file.txt | 1 +
	 1 file changed, 1 insertion(+)

	diff --git a/file.txt b/file.txt
	--- /dev/null
	+++ b/file.txt
	@@ -0,0 +1 @@
	+hello
	--
	2.0.0

	From nobody Mon Sep 17 00:00:01 2001
	From: Author Two <two@example.com>
	Subject: [PATCH 2/2] Second patch
	Date: Mon, 17 Sep 2001 00:00:01 +0000

	Second commit message body.

	---
	 file.txt | 2 +-
	 1 file changed, 1 insertion(+), 1 deletion(-)

	diff --git a/file.txt b/file.txt
	--- a/file.txt
	+++ b/file.txt
	@@ -1 +1 @@
	-hello
	+world
	--
	2.0.0

	MBOX
	mkdir -p split &&
	git mailsplit -osplit sample.mbox >last &&
	test "$(cat last)" = "2"
'

test_expect_success 'mailsplit produces individual files' '
	test_path_is_file split/0001 &&
	test_path_is_file split/0002
'

test_expect_success 'mailinfo extracts author and subject' '
	git mailinfo msg patch <split/0001 >info &&
	grep "Author One" info &&
	grep "First patch" info
'

test_expect_success 'mailinfo extracts email' '
	grep "one@example.com" info
'

test_expect_success 'mailinfo extracts message body' '
	grep "First commit message body" msg
'

test_expect_success 'mailinfo extracts patch' '
	grep "diff --git" patch
'

test_expect_success 'mailinfo on second message' '
	git mailinfo msg2 patch2 <split/0002 >info2 &&
	grep "Author Two" info2 &&
	grep "Second patch" info2 &&
	grep "two@example.com" info2
'

test_expect_success 'mailinfo --scissors' '
	cat >scissors-mail <<-\EOF &&
	From: Test Author <test@example.com>
	Subject: [PATCH] Scissors test

	Blah blah discussion text.

	-- >8 --
	Subject: [PATCH] Real subject

	Real commit message.

	---
	 file.txt | 1 +

	diff --git a/file.txt b/file.txt
	--- a/file.txt
	+++ b/file.txt
	@@ -1 +1 @@
	-old
	+new
	EOF
	git mailinfo --scissors smsg spatch <scissors-mail >sinfo &&
	grep "Real commit message" smsg
'

test_done
