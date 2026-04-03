#!/bin/sh

test_description='git apply with multi-file patches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo "file1 content" >file1 &&
	echo "file2 content" >file2 &&
	git add file1 file2 &&
	git commit -m initial
'

test_expect_success 'apply patch modifying multiple files' '
	cd repo &&
	cat >multi.patch <<-\EOF &&
	diff --git a/file1 b/file1
	--- a/file1
	+++ b/file1
	@@ -1 +1 @@
	-file1 content
	+file1 modified
	diff --git a/file2 b/file2
	--- a/file2
	+++ b/file2
	@@ -1 +1 @@
	-file2 content
	+file2 modified
	EOF
	git apply multi.patch &&
	echo "file1 modified" >expect &&
	test_cmp expect file1 &&
	echo "file2 modified" >expect &&
	test_cmp expect file2
'

test_expect_success 'apply patch with file creation and modification' '
	cd repo &&
	git checkout -- file1 file2 &&
	cat >mixed.patch <<-\EOF &&
	diff --git a/file1 b/file1
	--- a/file1
	+++ b/file1
	@@ -1 +1,2 @@
	 file1 content
	+extra line
	diff --git a/newfile b/newfile
	new file mode 100644
	--- /dev/null
	+++ b/newfile
	@@ -0,0 +1 @@
	+brand new
	EOF
	git apply mixed.patch &&
	test_write_lines "file1 content" "extra line" >expect &&
	test_cmp expect file1 &&
	echo "brand new" >expect &&
	test_cmp expect newfile
'

test_done
