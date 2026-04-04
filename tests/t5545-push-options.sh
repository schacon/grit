#!/bin/sh
# Ported from git/t/t5545-push-options.sh

test_description='pushing to a repository using push options'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

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

test_expect_failure 'one push option works for a single branch (grit: push options not forwarded to hooks)' '
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

test_expect_failure 'push option denied by remote (grit: receive.advertisePushOptions not supported)' '
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

test_expect_failure 'two push options work (grit: push options not forwarded to hooks)' '
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

test_expect_failure 'default push option (grit: push.pushOption config not supported)' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions true &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		git -c push.pushOption=default push up main
	) &&
	test_refs main main &&
	echo "default" >expect &&
	test_cmp expect upstream/.git/hooks/pre-receive.push_options &&
	test_cmp expect upstream/.git/hooks/post-receive.push_options
'

test_expect_failure 'two default push options (grit: push.pushOption config not supported)' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions true &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		git -c push.pushOption=default1 -c push.pushOption=default2 push up main
	) &&
	test_refs main main &&
	printf "default1\ndefault2\n" >expect &&
	test_cmp expect upstream/.git/hooks/pre-receive.push_options &&
	test_cmp expect upstream/.git/hooks/post-receive.push_options
'

test_expect_failure 'push option from command line overrides from-config push option (grit: push.pushOption config not supported)' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions true &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		git -c push.pushOption=default push --push-option=manual up main
	) &&
	test_refs main main &&
	echo "manual" >expect &&
	test_cmp expect upstream/.git/hooks/pre-receive.push_options &&
	test_cmp expect upstream/.git/hooks/post-receive.push_options
'

test_expect_failure 'empty value of push.pushOption in config clears the list (grit: push.pushOption config not supported)' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions true &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		git -c push.pushOption=default1 -c push.pushOption= -c push.pushOption=default2 push up main
	) &&
	test_refs main main &&
	echo "default2" >expect &&
	test_cmp expect upstream/.git/hooks/pre-receive.push_options &&
	test_cmp expect upstream/.git/hooks/post-receive.push_options
'

test_expect_failure 'invalid push option in config (grit: push.pushOption config not supported)' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions true &&
	(
		cd workbench &&
		test_commit one &&
		git push --mirror up &&
		test_commit two &&
		test_must_fail git -c push.pushOption push up main
	) &&
	test_refs main HEAD@{1}
'

test_expect_failure 'push options keep quoted characters intact (direct) (grit: push options not forwarded to hooks)' '
	mk_repo_pair &&
	git -C upstream config receive.advertisePushOptions true &&
	(
		cd workbench &&
		test_commit one &&
		git push --push-option="\"embedded quotes\"" up main
	) &&
	echo "\"embedded quotes\"" >expect &&
	test_cmp expect upstream/.git/hooks/pre-receive.push_options
'

# NOTE: HTTP tests from upstream are omitted as grit does not have HTTP test infrastructure

test_done
