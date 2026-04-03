#!/bin/sh

test_description='patching from inconvenient places'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo 1 >preimage &&
	printf "%s\n" 1 2 >postimage &&

	git commit --allow-empty -m basis
'

test_expect_success 'apply basic patch' '
	cat >root-patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1 +1,2 @@
	 1
	+2
	EOF
	cp preimage file &&
	git apply root-patch &&
	test_cmp postimage file
'

test_expect_success 'apply --check with valid patch' '
	cp preimage file &&
	git apply --check root-patch
'

test_expect_success 'apply --stat with valid patch' '
	git apply --stat root-patch >stat_output &&
	grep "file" stat_output
'

test_expect_success 'apply --numstat with valid patch' '
	git apply --numstat root-patch >numstat_output &&
	grep "1	0	file" numstat_output
'

test_expect_success 'apply --summary with new file patch' '
	cat >new-patch <<-\EOF &&
	diff --git a/newfile b/newfile
	new file mode 100644
	--- /dev/null
	+++ b/newfile
	@@ -0,0 +1 @@
	+new content
	EOF
	git apply new-patch &&
	echo "new content" >expected &&
	test_cmp expected newfile
'

test_expect_success 'apply patch that deletes a file' '
	echo "to delete" >deleteme &&
	cat >del-patch <<-\EOF &&
	diff --git a/deleteme b/deleteme
	deleted file mode 100644
	--- a/deleteme
	+++ /dev/null
	@@ -1 +0,0 @@
	-to delete
	EOF
	git apply del-patch &&
	test_path_is_missing deleteme
'

test_expect_success 'apply --reverse' '
	cp postimage file &&
	git apply -R root-patch &&
	test_cmp preimage file
'

test_done
