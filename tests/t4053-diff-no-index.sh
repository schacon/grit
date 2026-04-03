#!/bin/sh
test_description='grit diff --no-index (comparing non-git files)

Tests for the --no-index option which compares files without a repo.'

. ./test-lib.sh

test_expect_success 'diff --no-index between two files' '
	echo "file one" >a.txt &&
	echo "file two" >b.txt &&
	test_must_fail git diff --no-index a.txt b.txt >out &&
	grep "a\.txt" out &&
	grep "b\.txt" out
'

test_expect_success 'diff --no-index identical files exits 0' '
	echo "same" >x.txt &&
	echo "same" >y.txt &&
	git diff --no-index x.txt y.txt >out &&
	test_must_be_empty out
'

test_expect_success 'diff --no-index --stat' '
	echo "one" >p.txt &&
	echo "two" >q.txt &&
	test_must_fail git diff --no-index --stat p.txt q.txt >out &&
	grep "q\.txt" out
'

test_expect_success 'diff --no-index --name-only' '
	echo "one" >p.txt &&
	echo "two" >q.txt &&
	test_must_fail git diff --no-index --name-only p.txt q.txt >out &&
	grep "q\.txt" out
'

test_expect_success 'diff --no-index --exit-code' '
	echo "one" >p.txt &&
	echo "two" >q.txt &&
	test_expect_code 1 git diff --no-index --exit-code p.txt q.txt
'

test_expect_success 'diff --no-index works outside git repo' '
	mkdir -p no-repo &&
	echo "a" >no-repo/f1 &&
	echo "b" >no-repo/f2 &&
	test_expect_code 1 git diff --no-index no-repo/f1 no-repo/f2 >out 2>&1 &&
	test -s out
'

test_expect_success 'diff --no-index with /dev/null (new file)' '
	echo "content" >new.txt &&
	test_must_fail git diff --no-index /dev/null new.txt >out &&
	grep "new\.txt" out
'

test_expect_failure 'diff --no-index between directories' '
	mkdir -p dir1 dir2 &&
	echo "a" >dir1/f.txt &&
	echo "b" >dir2/f.txt &&
	test_must_fail git diff --no-index dir1 dir2 >out &&
	grep "f\.txt" out
'

test_expect_success 'diff --no-index --numstat' '
	echo "alpha" >m.txt &&
	echo "beta" >n.txt &&
	test_must_fail git diff --no-index --numstat m.txt n.txt >out &&
	grep "n\.txt" out
'

test_expect_success 'diff --no-index --quiet' '
	echo "alpha" >m.txt &&
	echo "beta" >n.txt &&
	test_expect_code 1 git diff --no-index --quiet m.txt n.txt
'

test_done
