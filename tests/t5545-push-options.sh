#!/bin/sh
# Ported from git/t/t5545-push-options.sh
# Tests pushing to a repository using push options
# Grit supports --push-option flag but does not pass GIT_PUSH_OPTION_*
# environment variables to hooks

test_description='pushing to a repository using push options'

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
		git config receive.denyCurrentBranch warn &&
		mkdir -p .git/hooks &&
		cat >.git/hooks/pre-receive <<-'EOF' &&
		#!/bin/sh
		if test -n "$GIT_PUSH_OPTION_COUNT"; then
			i=0
			>hooks/pre-receive.push_options
			while test "$i" -lt "$GIT_PUSH_OPTION_COUNT"; do
				eval "value=\$GIT_PUSH_OPTION_$i"
				echo $value >>hooks/pre-receive.push_options
				i=$((i + 1))
			done
		fi
		EOF
		chmod u+x .git/hooks/pre-receive

		cat >.git/hooks/post-receive <<-'EOF' &&
		#!/bin/sh
		if test -n "$GIT_PUSH_OPTION_COUNT"; then
			i=0
			>hooks/post-receive.push_options
			while test "$i" -lt "$GIT_PUSH_OPTION_COUNT"; do
				eval "value=\$GIT_PUSH_OPTION_$i"
				echo $value >>hooks/post-receive.push_options
				i=$((i + 1))
			done
		fi
		EOF
		chmod u+x .git/hooks/post-receive
	) &&
	(
		cd workbench &&
		git remote add up ../upstream
	)
}

test_refs () {
	test $# = 2 &&
	git -C upstream rev-parse --verify "$1" >expect &&
	git -C workbench rev-parse --verify "$2" >actual &&
	test_cmp expect actual
}

test_expect_success 'setup' '
	git init -q
'

# grit does not pass GIT_PUSH_OPTION_* to hooks
test_expect_success 'one push option works for a single branch' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions true &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		git push --push-option=asdf up main
	) &&
	test_refs main main &&
	echo "asdf" >expect &&
	test_cmp expect upstream/.git/hooks/pre-receive.push_options &&
	test_cmp expect upstream/.git/hooks/post-receive.push_options
'

# grit does not support receive.advertisePushOptions=false rejection
test_expect_failure 'push option denied by remote' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions false &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		test_must_fail git push --push-option=asdf up main
	) &&
	test_refs main HEAD@{1}
'

# grit does not pass GIT_PUSH_OPTION_* to hooks
test_expect_success 'two push options work' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions true &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		git push --push-option=asdf --push-option="more structured text" up main
	) &&
	test_refs main main &&
	printf "asdf\nmore structured text\n" >expect &&
	test_cmp expect upstream/.git/hooks/pre-receive.push_options &&
	test_cmp expect upstream/.git/hooks/post-receive.push_options
'

test_expect_success 'push with push-option flag accepted' '
	mk_repo_pair &&
	(
		cd workbench &&
		test_commit one &&
		git push --push-option=test up main
	) &&
	test_refs main main
'

test_done
