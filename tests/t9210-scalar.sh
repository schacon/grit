#!/bin/sh
#
# Upstream: t9210-scalar.sh
# Tests for the `scalar` command.
#

test_description='test the `scalar` command'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'scalar shows a usage' '
	test_expect_code 129 scalar -h
'

test_expect_success 'scalar invoked on enlistment root' '
	test_when_finished "rm -rf test src deeper" &&

	for enlistment_root in test src deeper/test
	do
		git init ${enlistment_root}/src &&

		# Register
		scalar register ${enlistment_root} &&
		scalar list >out &&
		grep "${enlistment_root}/src\$" out &&

		# Delete (including enlistment root)
		scalar delete $enlistment_root &&
		test_path_is_missing $enlistment_root &&
		scalar list >out &&
		! grep "${enlistment_root}/src\$" out || return 1
	done
'

test_expect_success 'scalar invoked on enlistment src repo' '
	test_when_finished "rm -rf test src deeper" &&

	for enlistment_root in test src deeper/test
	do
		git init ${enlistment_root}/src &&

		# Register
		scalar register ${enlistment_root}/src &&
		scalar list >out &&
		grep "${enlistment_root}/src\$" out &&

		# Delete (will not include enlistment root)
		scalar delete ${enlistment_root}/src &&
		test_path_is_dir $enlistment_root &&
		scalar list >out &&
		! grep "${enlistment_root}/src\$" out || return 1
	done
'

test_expect_success 'scalar invoked when enlistment root and repo are the same' '
	test_when_finished "rm -rf test src deeper" &&

	for enlistment_root in test src deeper/test
	do
		git init ${enlistment_root} &&

		# Register
		scalar register ${enlistment_root} &&
		scalar list >out &&
		grep "${enlistment_root}\$" out &&

		# Delete
		scalar delete ${enlistment_root} &&
		test_path_is_missing $enlistment_root &&
		scalar list >out &&
		! grep "${enlistment_root}\$" out &&

		# Make sure we did not accidentally delete the trash dir
		test_path_is_dir "$TRASH_DIRECTORY" || return 1
	done
'

test_expect_success 'scalar repo search respects GIT_CEILING_DIRECTORIES' '
	test_when_finished "rm -rf test" &&

	git init test/src &&
	mkdir -p test/src/deep &&
	GIT_CEILING_DIRECTORIES="$(pwd)/test/src" &&
	! scalar register test/src/deep 2>err &&
	grep "not a git repository" err
'

test_expect_success 'scalar enlistments need a worktree' '
	test_when_finished "rm -rf bare test" &&

	git init --bare bare/src &&
	! scalar register bare/src 2>err &&
	grep "Scalar enlistments require a worktree" err
'

test_expect_success 'scalar register warns when background maintenance fails' '
	test_when_finished "rm -rf register-repo" &&
	git init register-repo &&
	scalar register register-repo 2>err &&
	true
'

test_expect_success 'scalar unregister' '
	test_when_finished "rm -rf vanish" &&
	git init vanish/src &&
	scalar register vanish/src &&
	scalar list >scalar.repos &&
	grep -F "vanish/src" scalar.repos &&
	rm -rf vanish/src/.git &&
	scalar unregister vanish &&
	scalar list >scalar.repos &&
	! grep -F "vanish/src" scalar.repos &&

	# scalar unregister should be idempotent
	scalar unregister vanish
'

test_expect_success 'scalar register --no-maintenance' '
	test_when_finished "rm -rf register-no-maint" &&
	git init register-no-maint &&
	scalar register --no-maintenance register-no-maint 2>err &&
	scalar list >out &&
	grep "register-no-maint" out
'

test_expect_success 'set up repository to clone' '
	git init clone-source &&
	cd clone-source &&
	test_commit first &&
	test_commit second &&
	test_commit third
'

test_expect_success 'scalar clone' '
	test_when_finished "rm -rf cloned" &&
	scalar clone "file://$(pwd)/clone-source" cloned --single-branch &&
	(
		cd cloned/src &&
		git log --oneline >log_out &&
		test_file_not_empty log_out
	)
'

test_expect_success 'scalar delete without enlistment shows a usage' '
	test_expect_code 1 scalar delete 2>err &&
	test_file_not_empty err
'

test_expect_success 'scalar delete with enlistment' '
	scalar clone "file://$(pwd)/clone-source" to-delete --single-branch &&
	scalar delete to-delete &&
	test_path_is_missing to-delete
'

test_expect_success 'scalar supports -c/-C' '
	test_when_finished "scalar delete sub" &&
	git init sub &&
	scalar -C sub -c status.aheadBehind=bogus register 2>err &&
	test false = "$(git -C sub config gui.gcwarning)"
'

test_expect_success '`scalar [...] <dir>` errors out when dir is missing' '
	! scalar run config cloned 2>err &&
	test_file_not_empty err
'

test_expect_success 'scalar diagnose' '
	test_when_finished "rm -rf diag-repo" &&
	git init diag-repo &&
	(
		cd diag-repo &&
		test_commit initial
	) &&
	scalar diagnose diag-repo >out 2>err &&
	true
'

test_expect_success 'scalar reconfigure sets config' '
	test_when_finished "rm -rf reconf" &&
	git init reconf/src &&
	scalar register reconf &&
	git -C reconf/src config unset gui.gcwarning &&
	scalar reconfigure reconf &&
	test false = "$(git -C reconf/src config gui.gcwarning)"
'

test_expect_success 'scalar reconfigure --all' '
	test_when_finished "rm -rf ra1 ra2" &&
	git init ra1/src &&
	git init ra2/src &&
	scalar register ra1 &&
	scalar register ra2 &&
	git -C ra1/src config unset gui.gcwarning &&
	git -C ra2/src config unset gui.gcwarning &&
	scalar reconfigure -a &&
	test false = "$(git -C ra1/src config gui.gcwarning)" &&
	test false = "$(git -C ra2/src config gui.gcwarning)"
'

test_expect_success '`reconfigure -a` removes stale config entries' '
	test_when_finished "rm -rf stale" &&
	git init stale/src &&
	scalar register stale &&
	scalar list >scalar.repos &&
	grep stale scalar.repos &&
	rm -rf stale &&
	scalar reconfigure -a &&
	scalar list >scalar.repos &&
	! grep stale scalar.repos
'

test_done
