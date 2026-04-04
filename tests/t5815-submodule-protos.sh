#!/bin/sh
# Ported from git/t/t5815-submodule-protos.sh
# Tests for protocol restrictions with submodules

test_description='protocol restrictions with submodules'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'clone with submodule respects protocol.file.allow' '
	test_create_repo sub &&
	(cd sub && test_commit one) &&
	test_create_repo super &&
	(
		cd super &&
		git submodule add ../sub sub &&
		git commit -m "add submodule"
	) &&
	test_must_fail git -c protocol.file.allow=never \
		clone --recurse-submodules super clone-denied 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'fetch with submodule respects protocol.file.allow' '
	git clone super clone-for-fetch &&
	test_must_fail git -C clone-for-fetch -c protocol.file.allow=never fetch 2>err &&
	grep -i "not allowed" err
'

test_done
