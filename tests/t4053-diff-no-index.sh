#!/bin/sh
test_description='grit diff --no-index (comparing non-git files)

The --no-index option is not yet implemented in grit. All tests here
are expected failures that document the desired behavior for when it
is implemented.'

. ./test-lib.sh

test_expect_failure 'diff --no-index between two files (not implemented)' '
	echo "file one" >a.txt &&
	echo "file two" >b.txt &&
	test_must_fail git diff --no-index a.txt b.txt >out &&
	grep "a\.txt" out &&
	grep "b\.txt" out
'

test_expect_failure 'diff --no-index identical files exits 0 (not implemented)' '
	echo "same" >x.txt &&
	echo "same" >y.txt &&
	git diff --no-index x.txt y.txt >out &&
	test_must_be_empty out
'

test_expect_failure 'diff --no-index --stat (not implemented)' '
	echo "one" >p.txt &&
	echo "two" >q.txt &&
	test_must_fail git diff --no-index --stat p.txt q.txt >out &&
	grep "p\.txt" out
'

test_expect_failure 'diff --no-index --name-only (not implemented)' '
	echo "one" >p.txt &&
	echo "two" >q.txt &&
	test_must_fail git diff --no-index --name-only p.txt q.txt >out &&
	grep "p\.txt" out
'

test_expect_failure 'diff --no-index --exit-code (not implemented)' '
	echo "one" >p.txt &&
	echo "two" >q.txt &&
	test_expect_code 1 git diff --no-index --exit-code p.txt q.txt
'

test_expect_failure 'diff --no-index works outside git repo (not implemented)' '
	mkdir -p no-repo &&
	echo "a" >no-repo/f1 &&
	echo "b" >no-repo/f2 &&
	(cd no-repo && git diff --no-index f1 f2 >out 2>&1; test $? -ne 0)
'

test_expect_failure 'diff --no-index with /dev/null (new file) (not implemented)' '
	echo "content" >new.txt &&
	test_must_fail git diff --no-index /dev/null new.txt >out &&
	grep "new\.txt" out
'

test_expect_failure 'diff --no-index between directories (not implemented)' '
	mkdir -p dir1 dir2 &&
	echo "a" >dir1/f.txt &&
	echo "b" >dir2/f.txt &&
	test_must_fail git diff --no-index dir1 dir2 >out &&
	grep "f\.txt" out
'

test_expect_failure 'diff --no-index --numstat (not implemented)' '
	echo "alpha" >m.txt &&
	echo "beta" >n.txt &&
	test_must_fail git diff --no-index --numstat m.txt n.txt >out &&
	grep "m\.txt" out
'

test_expect_failure 'diff --no-index --quiet (not implemented)' '
	echo "alpha" >m.txt &&
	echo "beta" >n.txt &&
	test_expect_code 1 git diff --no-index --quiet m.txt n.txt
'

test_done
