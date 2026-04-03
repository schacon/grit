#!/bin/sh

test_description='git rev-list with various object types'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup well-formed objects' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	echo "foo" >file &&
	git add file &&
	git commit -m "first commit" &&
	blob=$(git rev-parse HEAD:file) &&
	echo "$blob" >../blob_oid &&
	tree=$(git rev-parse HEAD^{tree}) &&
	echo "$tree" >../tree_oid &&
	commit=$(git rev-parse HEAD) &&
	echo "$commit" >../commit_oid
'

test_expect_success 'rev-list --objects on commit' '
	cd repo &&
	commit=$(cat ../commit_oid) &&
	git rev-list --objects $commit >output &&
	test -s output
'

test_expect_success 'rev-list on tree fails' '
	cd repo &&
	tree=$(cat ../tree_oid) &&
	test_must_fail git rev-list $tree 2>err
'

test_expect_success 'rev-list --objects includes objects' '
	cd repo &&
	commit=$(cat ../commit_oid) &&
	git rev-list --objects $commit >output &&
	test $(wc -l < output) -ge 1
'

test_expect_success 'rev-list --count works' '
	cd repo &&
	git rev-list --count HEAD >output &&
	echo 1 >expect &&
	test_cmp expect output
'

test_done
