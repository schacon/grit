#!/bin/sh

test_description='git apply with cached and worktree modes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	test_write_lines 1 2 3 4 5 >file &&
	git add file &&
	git commit -m "commit" &&
	git tag base
'

test_expect_success 'apply creation patch to worktree' '
	cd repo &&
	cat >creation.patch <<-\EOF &&
	diff --git a/newfile b/newfile
	new file mode 100644
	--- /dev/null
	+++ b/newfile
	@@ -0,0 +1,5 @@
	+1
	+2
	+3
	+4
	+5
	EOF
	rm -f newfile &&
	git apply creation.patch &&
	test_write_lines 1 2 3 4 5 >expect &&
	test_cmp expect newfile
'

test_expect_success 'apply deletion patch to worktree' '
	cd repo &&
	cat >deletion.patch <<-\EOF &&
	diff --git a/newfile b/newfile
	deleted file mode 100644
	--- a/newfile
	+++ /dev/null
	@@ -1,5 +0,0 @@
	-1
	-2
	-3
	-4
	-5
	EOF
	git apply deletion.patch &&
	test_path_is_missing newfile
'

test_expect_success 'apply --cached creation patch' '
	cd repo &&
	cat >cached-create.patch <<-\EOF &&
	diff --git a/cached-file b/cached-file
	new file mode 100644
	--- /dev/null
	+++ b/cached-file
	@@ -0,0 +1 @@
	+cached content
	EOF
	git apply --cached cached-create.patch &&
	git ls-files --stage cached-file >output &&
	test_grep "cached-file" output &&
	git reset HEAD cached-file
'

test_done
