#!/bin/sh
#
# Ported from git/t/t7418-submodule-sparse-gitmodules.sh
# Tests reading/writing .gitmodules when not in the working tree
#

test_description='Test reading/writing .gitmodules when not in the working tree'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'sparse checkout setup which hides .gitmodules' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	"$REAL_GIT" init upstream &&
	"$REAL_GIT" init submodule &&
	(cd submodule &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo file >file &&
		"$REAL_GIT" add file &&
		test_tick &&
		"$REAL_GIT" commit -m "Add file"
	) &&
	(cd upstream &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" config protocol.file.allow always &&
		"$REAL_GIT" submodule add ../submodule &&
		test_tick &&
		"$REAL_GIT" commit -m "Add submodule"
	) &&
	"$REAL_GIT" clone --template= upstream super &&
	(cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" config protocol.file.allow always &&
		mkdir .git/info &&
		cat >.git/info/sparse-checkout <<-\EOF &&
		/*
		!/.gitmodules
		EOF
		"$REAL_GIT" config core.sparsecheckout true &&
		"$REAL_GIT" read-tree -m -u HEAD &&
		test_path_is_missing .gitmodules
	)
'

test_expect_failure 'initialising submodule when the gitmodules config is not checked out' '
	test_must_fail git -C super config submodule.submodule.url &&
	git -C super submodule init &&
	git -C super config submodule.submodule.url >actual &&
	echo "$(pwd)/submodule" >expect &&
	test_cmp expect actual
'

test_expect_failure 'updating submodule when the gitmodules config is not checked out' '
	test_path_is_missing super/submodule/file &&
	git -C super submodule update &&
	test_cmp submodule/file super/submodule/file
'

test_done
