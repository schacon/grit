#!/bin/sh

test_description='rev-list with pack files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_commit loose &&
	test_commit packed &&
	test_commit kept
'

test_expect_success 'rev-list all objects' '
	cd repo &&
	git rev-list --objects HEAD >output &&
	test -s output
'

test_expect_success 'pack-objects creates pack' '
	cd repo &&
	git rev-list HEAD | git pack-objects .git/objects/pack/pack >pack_name &&
	test -s pack_name
'

test_expect_success 'show-index reads pack index' '
	cd repo &&
	pack=$(cat pack_name) &&
	git show-index <.git/objects/pack/pack-${pack}.idx >index_output &&
	test -s index_output
'

test_done
