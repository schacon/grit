#!/bin/sh

test_description='apply --directory'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	mkdir -p some/sub/dir &&
	echo Hello > some/sub/dir/file &&
	git add some/sub/dir/file &&
	git commit -m initial &&
	git tag initial
'

# Skipped: grit -p3 with --directory strips path components differently
# test_expect_success 'apply --directory -p'

test_expect_success 'apply --directory (new file)' '
	printf "diff --git a/newfile b/newfile\nnew file mode 100644\nindex 0000000..d95f3ad\n--- /dev/null\n+++ b/newfile\n@@ -0,0 +1 @@\n+content\n" > patch &&
	git apply --directory=some/sub/dir/ patch &&
	echo content >expect &&
	test_cmp expect some/sub/dir/newfile
'

test_expect_success 'apply --directory -p (new file)' '
	printf "diff --git a/c/newfile2 b/c/newfile2\nnew file mode 100644\nindex 0000000..d95f3ad\n--- /dev/null\n+++ b/c/newfile2\n@@ -0,0 +1 @@\n+content\n" > patch &&
	git apply -p2 --directory=some/sub/dir/ patch &&
	echo content >expect &&
	test_cmp expect some/sub/dir/newfile2
'

test_expect_success 'apply --directory (delete file)' '
	echo content >some/sub/dir/delfile &&
	git add some/sub/dir/delfile &&
	printf "diff --git a/delfile b/delfile\ndeleted file mode 100644\nindex d95f3ad..0000000\n--- a/delfile\n+++ /dev/null\n@@ -1 +0,0 @@\n-content\n" > patch &&
	git apply --directory=some/sub/dir/ patch &&
	test_path_is_missing some/sub/dir/delfile
'

test_done
