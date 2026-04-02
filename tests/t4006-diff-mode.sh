#!/bin/sh
test_description='grit diff file mode changes (chmod +x)'

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "plain file" >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

test_expect_success 'diff detects mode change (100644 -> 100755)' '
	cd repo &&
	chmod +x file.txt &&
	git diff >out &&
	grep "old mode 100644" out &&
	grep "new mode 100755" out
'

test_expect_success 'diff --stat for mode-only change does not crash' '
	cd repo &&
	chmod +x file.txt &&
	git diff --stat >out &&
	test -f out
'

test_expect_success 'diff --name-only shows mode-changed file' '
	cd repo &&
	chmod +x file.txt &&
	git diff --name-only >out &&
	grep "file.txt" out
'

test_expect_success 'diff --name-status shows mode change' '
	cd repo &&
	chmod +x file.txt &&
	git diff --name-status >out &&
	grep "file.txt" out
'

test_expect_success 'diff --exit-code detects mode change' '
	cd repo &&
	chmod +x file.txt &&
	test_must_fail git diff --exit-code
'

test_expect_success 'diff --quiet detects mode change' '
	cd repo &&
	chmod +x file.txt &&
	test_must_fail git diff --quiet
'

test_expect_success 'setup for mode+content combo' '
	cd repo &&
	git init combo &&
	cd combo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "line1" >f.txt &&
	git add f.txt &&
	git commit -m "first" &&
	chmod +x f.txt &&
	echo "line2" >>f.txt &&
	git diff >../combo_out
'

test_expect_success 'diff shows mode change with content change' '
	cd repo &&
	grep "old mode 100644" combo_out &&
	grep "new mode 100755" combo_out &&
	grep "f.txt" combo_out
'

test_expect_success 'diff --cached detects staged mode change' '
	cd repo &&
	chmod +x file.txt &&
	git add file.txt &&
	git diff --cached >out &&
	grep "old mode 100644" out &&
	grep "new mode 100755" out
'

test_expect_success 'diff between commits with mode change' '
	cd repo &&
	git commit -m "make executable" &&
	c1=$(git rev-parse HEAD~1) &&
	c2=$(git rev-parse HEAD) &&
	git diff-tree -r "$c1" "$c2" >out &&
	grep "100644" out &&
	grep "100755" out &&
	grep "file.txt" out
'

test_expect_success 'diff-tree --name-status for mode change' '
	cd repo &&
	c1=$(git rev-parse HEAD~1) &&
	c2=$(git rev-parse HEAD) &&
	git diff-tree -r --name-status "$c1" "$c2" >out &&
	grep "file.txt" out
'

# --- additional mode change tests ---

test_expect_success 'diff --numstat for mode-only change' '
	cd repo &&
	chmod -x file.txt &&
	git add file.txt &&
	git commit -m "remove exec" &&
	chmod +x file.txt &&
	git diff --numstat >out &&
	grep "file.txt" out
'

test_expect_success 'diff --exit-code returns 0 after resetting mode' '
	cd repo &&
	git checkout -- file.txt &&
	git diff --exit-code
'

test_expect_success 'diff --stat for mode change between commits' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	git diff --stat "$c1" "$c2" >out &&
	grep "file.txt" out
'

test_expect_success 'diff between commits shows old mode and new mode' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	git diff "$c1" "$c2" >out &&
	grep "old mode" out &&
	grep "new mode" out
'

test_expect_success 'diff --name-only between commits with mode change' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	git diff --name-only "$c1" "$c2" >out &&
	grep "file.txt" out
'

test_expect_success 'diff --name-status between commits with mode change' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	git diff --name-status "$c1" "$c2" >out &&
	grep "file.txt" out
'

test_expect_success 'diff --numstat between commits with mode change' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	git diff --numstat "$c1" "$c2" >out &&
	grep "file.txt" out
'

test_expect_success 'diff --exit-code detects mode change between commits' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	test_must_fail git diff --exit-code "$c1" "$c2"
'

test_expect_success 'diff --quiet detects mode change between commits' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	test_must_fail git diff --quiet "$c1" "$c2"
'

test_done
