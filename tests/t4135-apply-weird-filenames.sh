#!/bin/sh

test_description='git apply with various file operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	test_tick &&
	git commit --allow-empty -m preimage &&
	git tag preimage
'

test_expect_success 'apply git-style file creation patch' '
	cd repo &&
	cat >create.diff <<-\EOF &&
	diff --git a/postimage.txt b/postimage.txt
	new file mode 100644
	--- /dev/null
	+++ b/postimage.txt
	@@ -0,0 +1 @@
	+postimage
	EOF
	echo postimage >expected &&
	rm -f postimage.txt &&
	git apply create.diff &&
	test_cmp expected postimage.txt
'

test_expect_success 'apply patch modifying existing file' '
	cd repo &&
	echo preimage >postimage.txt &&
	cat >modify.diff <<-\EOF &&
	diff --git a/postimage.txt b/postimage.txt
	--- a/postimage.txt
	+++ b/postimage.txt
	@@ -1 +1 @@
	-preimage
	+postimage
	EOF
	echo postimage >expected &&
	git apply modify.diff &&
	test_cmp expected postimage.txt
'

test_expect_success 'apply patch with file deletion via -p2' '
	cd repo &&
	echo content >Makefile &&
	cat >svn.diff <<-\EOF &&
	diff --git a/branches/Makefile
	deleted file mode 100644
	--- a/branches/Makefile
	+++ /dev/null
	@@ -1 +0,0 @@
	-content
	EOF
	git apply -p2 svn.diff &&
	test_path_is_missing Makefile
'

test_done
