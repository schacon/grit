#!/bin/sh

test_description='git apply handling rename patch.'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success setup '
	echo "This is foo" >foo &&
	git update-index --add foo
'

test_expect_success 'apply stat and summary on rename patch' '
	cat >test-patch <<-\EOF &&
	diff --git a/foo b/bar
	similarity index 47%
	rename from foo
	rename to bar
	--- a/foo
	+++ b/bar
	@@ -1 +1 @@
	-This is foo
	+This is bar
	EOF
	git apply --stat test-patch >output &&
	test -s output &&
	git apply --summary test-patch >output &&
	test -s output
'

test_expect_success 'apply simple modification patch' '
	echo "original content" >testfile &&
	git add testfile &&
	git commit -m "add testfile" &&
	cat >mod-patch <<-\EOF &&
	diff --git a/testfile b/testfile
	--- a/testfile
	+++ b/testfile
	@@ -1 +1 @@
	-original content
	+modified content
	EOF
	git apply mod-patch &&
	grep "modified content" testfile
'

test_expect_success 'apply reverse modification patch' '
	git apply -R mod-patch &&
	grep "original content" testfile
'

test_done
