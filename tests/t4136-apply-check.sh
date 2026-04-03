#!/bin/sh

test_description='git apply --check and --stat basics'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	echo 1 >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'apply --check succeeds on valid patch' '
	cd repo &&
	cat >valid.patch <<-\EOF &&
	diff --git a/file b/file
	index d00491f..0cfbf08 100644
	--- a/file
	+++ b/file
	@@ -1 +1 @@
	-1
	+2
	EOF
	git apply --check valid.patch
'

test_expect_success 'apply --stat shows diffstat' '
	cd repo &&
	git apply --stat valid.patch >output &&
	test_grep "1 file changed" output
'

test_expect_success 'apply --numstat shows numbers' '
	cd repo &&
	git apply --numstat valid.patch >output &&
	test_grep "1	1	file" output
'

test_expect_success 'apply creates new file' '
	cd repo &&
	cat >create.patch <<-\EOF &&
	diff --git a/newfile b/newfile
	new file mode 100644
	index 0000000..d00491f
	--- /dev/null
	+++ b/newfile
	@@ -0,0 +1 @@
	+1
	EOF
	git apply create.patch &&
	test -f newfile &&
	echo 1 >expect &&
	test_cmp expect newfile
'

test_expect_success 'apply deletes file' '
	cd repo &&
	cat >delete.patch <<-\EOF &&
	diff --git a/newfile b/newfile
	deleted file mode 100644
	index d00491f..0000000
	--- a/newfile
	+++ /dev/null
	@@ -1 +0,0 @@
	-1
	EOF
	git apply delete.patch &&
	test_path_is_missing newfile
'

test_expect_success 'apply --reverse undoes a patch' '
	cd repo &&
	cat >forward.patch <<-\EOF &&
	diff --git a/file b/file
	index d00491f..0cfbf08 100644
	--- a/file
	+++ b/file
	@@ -1 +1 @@
	-1
	+2
	EOF
	git apply forward.patch &&
	echo 2 >expect &&
	test_cmp expect file &&
	git apply -R forward.patch &&
	echo 1 >expect &&
	test_cmp expect file
'

test_expect_success 'apply --cached modifies index' '
	cd repo &&
	cat >cached.patch <<-\EOF &&
	diff --git a/file b/file
	index d00491f..0cfbf08 100644
	--- a/file
	+++ b/file
	@@ -1 +1 @@
	-1
	+2
	EOF
	git apply --cached cached.patch &&
	git diff --cached >output &&
	test_grep "+2" output &&
	git reset --hard HEAD
'

test_expect_success 'apply mode change' '
	cd repo &&
	cat >mode.patch <<-\EOF &&
	diff --git a/file b/file
	old mode 100644
	new mode 100755
	EOF
	git apply mode.patch &&
	test -x file
'

test_done
