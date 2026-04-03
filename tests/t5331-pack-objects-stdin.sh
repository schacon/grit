#!/bin/sh

test_description='pack-objects stdin operations'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit A &&
	test_commit B &&
	test_commit C
'

test_expect_success 'pack-objects reads object list from stdin' '
	git rev-parse HEAD HEAD~1 HEAD~2 >oids &&
	git pack-objects testpack <oids &&
	git verify-pack testpack-*.pack
'

test_expect_success 'pack-objects --revs reads revisions from stdin' '
	echo HEAD | git pack-objects --revs --stdout >revs.pack &&
	git index-pack --stdin <revs.pack
'

test_expect_success 'pack-objects --all packs everything' '
	git pack-objects --all allpack &&
	git verify-pack -v allpack-*.pack >output &&
	test_grep "commit" output &&
	test_grep "tree" output &&
	test_grep "blob" output
'

test_expect_success 'pack-objects with duplicate input' '
	oid=$(git rev-parse HEAD) &&
	printf "%s\n%s\n" "$oid" "$oid" >dups &&
	git pack-objects duppack <dups &&
	git verify-pack duppack-*.pack
'

test_done
