#!/bin/sh
#
# Ported subset from git/t/t4132-apply-removal.sh
# Tests git apply for file creation and deletion via git-format diffs

test_description='git apply handles creation and deletion patches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	echo something >something
'

test_expect_success 'apply file creation patch' '
	cd repo &&
	rm -f file &&
	cat >create.patch <<-\EOF &&
	diff --git a/file b/file
	new file mode 100644
	index 0000000..deba01f
	--- /dev/null
	+++ b/file
	@@ -0,0 +1 @@
	+something
	EOF
	git apply create.patch &&
	test_cmp file something
'

test_expect_success 'apply file deletion patch' '
	cd repo &&
	echo something >file &&
	git add file &&
	git commit -m "add file" &&
	cat >remove.patch <<-\EOF &&
	diff --git a/file b/file
	deleted file mode 100644
	index deba01f..0000000
	--- a/file
	+++ /dev/null
	@@ -1 +0,0 @@
	-something
	EOF
	git apply remove.patch &&
	test_path_is_missing file
'

test_expect_success 'apply add-content patch to empty file' '
	cd repo &&
	>file &&
	git add file &&
	git commit -m "empty file" &&
	cat >add.patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -0,0 +1 @@
	+something
	EOF
	git apply add.patch &&
	test_cmp file something
'

test_expect_success 'apply patch to make file empty' '
	cd repo &&
	echo something >file &&
	git add file &&
	git commit -m "file with content" &&
	cat >empty.patch <<-\EOF &&
	diff --git a/file b/file
	--- a/file
	+++ b/file
	@@ -1 +0,0 @@
	-something
	EOF
	git apply empty.patch &&
	test -f file &&
	test_must_be_empty file
'

test_done
