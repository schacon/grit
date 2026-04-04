#!/bin/sh
# Ported from git/t/t5543-atomic-push.sh
# Tests pushing using --atomic option

test_description='pushing to a repository using the atomic push option'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

mk_repo_pair () {
	rm -rf workbench upstream &&
	test_create_repo upstream &&
	test_create_repo workbench &&
	(
		cd upstream &&
		git config receive.denyCurrentBranch warn
	) &&
	(
		cd workbench &&
		git remote add up ../upstream
	)
}

# Compare the ref ($1) in upstream with a ref value from workbench ($2)
test_refs () {
	test $# = 2 &&
	git -C upstream rev-parse --verify "$1" >expect &&
	git -C workbench rev-parse --verify "$2" >actual &&
	test_cmp expect actual
}

test_expect_success 'setup' '
	git init -q
'

test_expect_success 'atomic push works for a single branch' '
	mk_repo_pair &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		git push --atomic up main
	) &&
	test_refs main main
'

test_expect_success 'atomic push works for two branches' '
	mk_repo_pair &&
	(
		cd workbench &&
		test_commit one &&
		git branch second &&
		git push --mirror up &&
		test_commit two &&
		git checkout second &&
		test_commit three &&
		git push --atomic up main second
	) &&
	test_refs main main &&
	test_refs second second
'

test_expect_success 'atomic push works in combination with --mirror' '
	mk_repo_pair &&
	(
		cd workbench &&
		test_commit one &&
		git checkout -b second &&
		test_commit two &&
		git push --atomic --mirror up
	) &&
	test_refs main main &&
	test_refs second second
'

test_expect_success 'atomic push works in combination with --force' '
	mk_repo_pair &&
	(
		cd workbench &&
		test_commit one &&
		git branch second main &&
		test_commit two_a &&
		git checkout second &&
		test_commit two_b &&
		test_commit three_b &&
		test_commit four &&
		git push --mirror up &&
		git checkout main &&
		test_commit three_a &&
		git checkout second &&
		git reset --hard HEAD^ &&
		git push --force --atomic up main second
	) &&
	test_refs main main &&
	test_refs second second
'

# grit does not support --all with push (flag is rejected)
# The intended test is that atomic push should fail when one branch
# has a non-fast-forward. We test this with explicit refspecs instead.
test_expect_success 'atomic push fails if one branch fails (explicit refs)' '
	mk_repo_pair &&
	(
		cd workbench &&
		test_commit one &&
		git checkout -b second main &&
		test_commit two &&
		test_commit three &&
		test_commit four &&
		git push --mirror up &&
		git reset --hard HEAD~2 &&
		test_commit five &&
		git checkout main &&
		test_commit six &&
		test_must_fail git push --atomic up main second
	)
'

# grit does not run update hooks during atomic push
test_expect_failure 'atomic push obeys update hook preventing a branch' '
	mk_repo_pair &&
	(
		cd workbench &&
		test_commit one &&
		git checkout -b second main &&
		test_commit two &&
		git push --mirror up
	) &&
	test_hook -C upstream update <<-\EOF &&
	# only allow update to main from now on
	test "$1" = "refs/heads/main"
	EOF
	(
		cd workbench &&
		git checkout main &&
		test_commit three &&
		git checkout second &&
		test_commit four &&
		test_must_fail git push --atomic up main second
	) &&
	test_refs main HEAD@{3} &&
	test_refs second HEAD@{1}
'

# grit receive.advertiseatomic config not supported
test_expect_failure 'atomic push is not advertised if configured' '
	mk_repo_pair &&
	(
		cd upstream &&
		git config receive.advertiseatomic 0
	) &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		test_must_fail git push --atomic up main
	) &&
	test_refs main HEAD@{1}
'

test_done
