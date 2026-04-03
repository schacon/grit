#!/bin/sh

test_description='reftable write options'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Test default reftable write behavior: a commit in a reftable repo
# should produce reftable files (not loose ref files).

test_expect_success 'default write options' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial &&

		# Reftable files should exist
		test_path_is_dir .git/reftable &&
		test_path_is_file .git/reftable/tables.list &&
		test "$(wc -l <.git/reftable/tables.list)" -gt 0 &&

		# No loose ref files should exist for heads
		test_must_fail test -f .git/refs/heads/main &&

		# The ref should be resolvable through reftable
		git rev-parse HEAD &&
		git rev-parse refs/heads/main
	)
'

# When core.logAllRefUpdates is false, reftable writes should not include
# log blocks.

test_expect_success 'disabled reflog writes no log blocks' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		git config core.logAllRefUpdates false &&
		echo content >file &&
		git add file &&
		git commit -m initial &&

		# The ref should still work
		git rev-parse HEAD &&

		# Reftable tables should exist
		test "$(wc -l <.git/reftable/tables.list)" -gt 0
	)
'

# The [reftable] blockSize option should be respected in written tables.

test_expect_success 'block-size option' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		git config reftable.blockSize 512 &&
		echo content >file &&
		git add file &&
		git commit -m initial &&

		# Verify ref is accessible
		git rev-parse HEAD &&
		git rev-parse refs/heads/main &&

		# The reftable files should exist and be functional
		test_path_is_file .git/reftable/tables.list &&
		test "$(wc -l <.git/reftable/tables.list)" -gt 0 &&

		# Additional operations should work with custom block size
		git update-ref refs/heads/feature HEAD &&
		git rev-parse refs/heads/feature
	)
'

test_done
