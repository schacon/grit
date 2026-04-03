#!/bin/sh
#
# Ported subset from git/t/t4133-apply-filenames.sh
# Tests git apply with various file creation patterns

test_description='git apply filename handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	echo content >existing &&
	git add existing &&
	git commit -m initial
'

test_expect_success 'apply creates new file via git diff format' '
	cd repo &&
	cat >new.patch <<-\EOF &&
	diff --git a/newfile b/newfile
	new file mode 100644
	index 0000000..d95f3ad
	--- /dev/null
	+++ b/newfile
	@@ -0,0 +1 @@
	+content
	EOF
	git apply new.patch &&
	echo content >expect &&
	test_cmp expect newfile
'

test_expect_success 'apply modifies existing file' '
	cd repo &&
	rm -f newfile &&
	cat >modify.patch <<-\EOF &&
	diff --git a/existing b/existing
	--- a/existing
	+++ b/existing
	@@ -1 +1 @@
	-content
	+modified
	EOF
	git apply modify.patch &&
	echo modified >expect &&
	test_cmp expect existing
'

test_expect_success 'apply --check does not modify files' '
	cd repo &&
	git checkout -- existing &&
	cat >check.patch <<-\EOF &&
	diff --git a/existing b/existing
	--- a/existing
	+++ b/existing
	@@ -1 +1 @@
	-content
	+checked
	EOF
	git apply --check check.patch &&
	echo content >expect &&
	test_cmp expect existing
'

test_done
