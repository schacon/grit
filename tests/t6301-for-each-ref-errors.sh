#!/bin/sh
# Ported subset from git/t/t6301-for-each-ref-errors.sh.

test_description='for-each-ref error handling'

. ./test-lib.sh

test_expect_success 'setup repo and baseline output' '
	git init repo &&
	cd repo &&
	EMPTY_TREE=$(printf "" | git hash-object -w -t tree --stdin) &&
	A=$(git commit-tree "$EMPTY_TREE" -m A) &&
	git update-ref refs/heads/main "$A" &&
	git for-each-ref --format="%(refname)" >full-list &&
	git for-each-ref --format="%(objectname) %(refname)" >brief-list
'

test_expect_success 'broken loose ref emits warning and is ignored' '
	cd repo &&
	: >.git/refs/heads/bogus &&
	echo "warning: ignoring broken ref refs/heads/bogus" >expect-err &&
	git for-each-ref --format="%(refname)" >out 2>err &&
	test_cmp full-list out &&
	test_cmp expect-err err &&
	rm -f .git/refs/heads/bogus
'

test_expect_success 'zero oid ref emits warning and is ignored' '
	cd repo &&
	rm -f .git/refs/heads/bogus &&
	echo 0000000000000000000000000000000000000000 >.git/refs/heads/zeros &&
	echo "warning: ignoring broken ref refs/heads/zeros" >expect-err &&
	git for-each-ref --format="%(refname)" >out 2>err &&
	test_cmp full-list out &&
	test_cmp expect-err err &&
	rm -f .git/refs/heads/zeros
'

test_expect_success 'missing object is fatal for default format' '
	cd repo &&
	rm -f .git/refs/heads/bogus .git/refs/heads/zeros &&
	MISSING=1111111111111111111111111111111111111111 &&
	git update-ref refs/heads/missing "$MISSING" &&
	test_must_fail git for-each-ref >out 2>err &&
	echo "fatal: missing object $MISSING for refs/heads/missing" >expect-err &&
	test_cmp expect-err err
'

test_expect_success 'missing object still lists in objectname-only format' '
	cd repo &&
	MISSING=1111111111111111111111111111111111111111 &&
	cat brief-list >expect &&
	echo "$MISSING refs/heads/missing" >>expect &&
	sort -k 2 expect >expect-sorted &&
	git for-each-ref --format="%(objectname) %(refname)" >actual &&
	sort -k 2 actual >actual-sorted &&
	test_cmp expect-sorted actual-sorted
'

test_done
