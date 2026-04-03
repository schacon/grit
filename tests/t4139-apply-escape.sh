#!/bin/sh

test_description='basic git apply path handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo
'

test_expect_success 'apply creates new file' '
	cd repo &&
	rm -f foo &&
	cat >patch <<-\EOF &&
	diff --git a/foo b/foo
	new file mode 100644
	index 0000000..53c74cd
	--- /dev/null
	+++ b/foo
	@@ -0,0 +1 @@
	+evil
	EOF
	git apply patch &&
	echo evil >expect &&
	test_cmp expect foo
'

test_expect_success 'apply deletes file' '
	cd repo &&
	echo evil >foo &&
	cat >patch <<-\EOF &&
	diff --git a/foo b/foo
	deleted file mode 100644
	index 53c74cd..0000000
	--- a/foo
	+++ /dev/null
	@@ -1 +0,0 @@
	-evil
	EOF
	git apply patch &&
	test_path_is_missing foo
'

test_expect_success 'apply --check validates cleanly' '
	cd repo &&
	echo original >bar &&
	git add bar &&
	git commit -m "add bar" &&
	cat >patch <<-\EOF &&
	diff --git a/bar b/bar
	--- a/bar
	+++ b/bar
	@@ -1 +1 @@
	-original
	+modified
	EOF
	git apply --check patch
'

test_expect_success 'apply --directory prepends path' '
	cd repo &&
	mkdir -p subdir &&
	echo original >subdir/bar &&
	git add subdir/bar &&
	git commit -m "add subdir/bar" &&
	cat >patch <<-\EOF &&
	diff --git a/bar b/bar
	--- a/bar
	+++ b/bar
	@@ -1 +1 @@
	-original
	+modified
	EOF
	git apply --directory=subdir patch &&
	echo modified >expect &&
	test_cmp expect subdir/bar
'

test_done
