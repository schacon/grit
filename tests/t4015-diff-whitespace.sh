#!/bin/sh
test_description='grit diff whitespace handling

Tests whitespace-related diff behavior. Many upstream whitespace options
(-w, -b, --ignore-space-at-eol, etc.) are not yet implemented in grit;
those are marked as expected failures.'

. ./test-lib.sh

# ---- Tests for trailing whitespace ----

test_expect_success 'setup trailing-ws repo' '
	git init trailing-ws &&
	cd trailing-ws &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	printf "line1\nline2\nline3\n" >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

test_expect_success 'diff detects trailing whitespace addition' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff >out &&
	grep "file\.txt" out &&
	grep "line1" out
'

test_expect_success 'diff --stat for trailing whitespace change' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff --stat >out &&
	grep "file\.txt" out
'

test_expect_success 'diff --numstat for trailing whitespace change' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff --numstat >out &&
	grep "file\.txt" out
'

test_expect_success 'diff --exit-code detects trailing whitespace' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	test_must_fail git diff --exit-code
'

test_expect_success 'diff --quiet detects trailing whitespace' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	test_must_fail git diff --quiet
'

test_expect_success 'diff --name-only for trailing whitespace change' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff --name-only >out &&
	grep "file\.txt" out
'

# ---- Tests for tab/space changes ----

test_expect_success 'setup tab repo' '
	git init tab-repo &&
	cd tab-repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	printf "line1\n\tindented\nline3\n" >file.txt &&
	git add file.txt &&
	git commit -m "initial with tabs"
'

test_expect_success 'diff shows tab-to-space change' '
	cd tab-repo &&
	printf "line1\n    indented\nline3\n" >file.txt &&
	git diff >out &&
	grep "indented" out
'

test_expect_success 'diff --exit-code detects tab change' '
	cd tab-repo &&
	printf "line1\n    indented\nline3\n" >file.txt &&
	test_must_fail git diff --exit-code
'

# ---- Tests for blank line changes ----

test_expect_success 'setup blank-line repo' '
	git init blank-repo &&
	cd blank-repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	printf "line1\nline2\nline3\n" >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

test_expect_success 'diff shows blank line insertion' '
	cd blank-repo &&
	printf "line1\n\nline2\nline3\n" >file.txt &&
	git diff >out &&
	grep "file\.txt" out
'

test_expect_success 'diff --exit-code detects blank line change' '
	cd blank-repo &&
	printf "line1\n\nline2\nline3\n" >file.txt &&
	test_must_fail git diff --exit-code
'

# ---- Tests for space-in-middle changes ----

test_expect_success 'setup middle-space repo' '
	git init middle-repo &&
	cd middle-repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	printf "ab\ncd\nef\n" >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

test_expect_success 'diff detects space-in-middle change' '
	cd middle-repo &&
	printf "a b\ncd\nef\n" >file.txt &&
	test_must_fail git diff --exit-code
'

test_expect_success 'diff shows space-in-middle in output' '
	cd middle-repo &&
	printf "a b\ncd\nef\n" >file.txt &&
	git diff >out &&
	grep "file\.txt" out
'

# ---- Context line tests with whitespace ----

test_expect_success 'diff -U0 still shows whitespace changes' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff -U0 >out &&
	grep "line1" out
'

test_expect_success 'diff -U1 shows limited context with whitespace' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff -U1 >out &&
	grep "file\.txt" out
'

# ---- Whitespace in committed diffs (diff-tree) ----

test_expect_success 'setup committed whitespace changes' '
	git init committed-ws &&
	cd committed-ws &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	printf "line1\nline2\nline3\n" >file.txt &&
	git add file.txt &&
	git commit -m "initial" &&
	printf "line1  \nline2\nline3  \n" >file.txt &&
	git add file.txt &&
	git commit -m "add trailing ws" &&
	git rev-parse HEAD~1 >../ws_c1 &&
	git rev-parse HEAD >../ws_c2
'

test_expect_success 'diff-tree detects whitespace changes between commits' '
	cd committed-ws &&
	c1=$(cat ../ws_c1) && c2=$(cat ../ws_c2) &&
	git diff-tree -r --name-only "$c1" "$c2" >out &&
	grep "file\.txt" out
'

test_expect_success 'diff-tree --name-status shows M for whitespace change' '
	cd committed-ws &&
	c1=$(cat ../ws_c1) && c2=$(cat ../ws_c2) &&
	git diff-tree -r --name-status "$c1" "$c2" >out &&
	grep "file\.txt" out
'

# ---- Upstream whitespace options (not yet implemented, expected failures) ----

test_expect_failure 'diff -w ignores all whitespace (not implemented)' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff -w >out &&
	test_must_be_empty out
'

test_expect_failure 'diff -b ignores space amount changes (not implemented)' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff -b >out &&
	test_must_be_empty out
'

test_expect_failure 'diff --ignore-space-at-eol (not implemented)' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff --ignore-space-at-eol >out &&
	test_must_be_empty out
'

test_expect_failure 'diff --ignore-blank-lines (not implemented)' '
	cd blank-repo &&
	printf "line1\n\nline2\nline3\n" >file.txt &&
	git diff --ignore-blank-lines >out &&
	test_must_be_empty out
'

test_expect_failure 'diff --ignore-all-space (not implemented)' '
	cd middle-repo &&
	printf "a b\ncd\nef\n" >file.txt &&
	git diff --ignore-all-space >out &&
	test_must_be_empty out
'

test_expect_failure 'diff -w with --stat (not implemented)' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff -w --stat >out &&
	test_must_be_empty out
'

test_expect_failure 'diff -w with --exit-code returns 0 (not implemented)' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff -w --exit-code
'

test_expect_failure 'diff -b collapses multiple spaces (not implemented)' '
	git init collapse-repo &&
	cd collapse-repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	printf "a  b\n" >f.txt &&
	git add f.txt &&
	git commit -m "spaces" &&
	printf "a b\n" >f.txt &&
	git diff -b >out &&
	test_must_be_empty out
'

test_expect_failure 'diff --ignore-cr-at-eol (not implemented)' '
	cd trailing-ws &&
	printf "line1\r\nline2\nline3\n" >file.txt &&
	git diff --ignore-cr-at-eol >out &&
	test_must_be_empty out
'

test_expect_failure 'diff --ignore-space-change (not implemented)' '
	cd trailing-ws &&
	printf "line1  \nline2\nline3\n" >file.txt &&
	git diff --ignore-space-change >out &&
	test_must_be_empty out
'

test_done
