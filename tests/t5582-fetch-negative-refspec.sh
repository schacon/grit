#!/bin/sh
# Ported from git/t/t5582-fetch-negative-refspec.sh
# Copyright (c) 2020, Jacob Keller.

test_description='"git fetch" with negative refspecs.

'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success setup '
	git init &&
	echo >file original &&
	git add file &&
	git commit -a -m original
'

test_expect_success "clone and setup child repos" '
	git clone . one &&
	(
		cd one &&
		echo >file updated by one &&
		git commit -a -m "updated by one" &&
		git checkout -b alternate &&
		echo >file updated again by one &&
		git commit -a -m "updated by one again" &&
		git checkout main
	) &&
	git clone . two &&
	(
		cd two &&
		git config branch.main.remote one &&
		git config remote.one.url ../one/.git/ &&
		git config remote.one.fetch +refs/heads/*:refs/remotes/one/* &&
		git config --add remote.one.fetch ^refs/heads/alternate
	) &&
	git clone . three
'

test_expect_failure "fetch one (grit: negative refspec in config not supported)" '
	echo >file updated by origin &&
	git commit -a -m "updated by origin" &&
	(
		cd two &&
		test_must_fail git rev-parse --verify refs/remotes/one/alternate &&
		git fetch one &&
		test_must_fail git rev-parse --verify refs/remotes/one/alternate &&
		git rev-parse --verify refs/remotes/one/main &&
		mine=$(git rev-parse refs/remotes/one/main) &&
		his=$(cd ../one && git rev-parse refs/heads/main) &&
		test "z$mine" = "z$his"
	)
'

test_expect_failure "fetch with negative refspec on commandline (grit: negative refspec ^ref not supported)" '
	echo >file updated by origin again &&
	git commit -a -m "updated by origin again" &&
	(
		cd three &&
		alternate_in_one=$(cd ../one && git rev-parse refs/heads/alternate) &&
		echo $alternate_in_one >expect &&
		git fetch ../one/.git refs/heads/*:refs/remotes/one/* ^refs/heads/main &&
		cut -f -1 .git/FETCH_HEAD >actual &&
		test_cmp expect actual
	)
'

test_expect_success "fetch with negative sha1 refspec fails" '
	echo >file updated by origin yet again &&
	git commit -a -m "updated by origin yet again" &&
	(
		cd three &&
		main_in_one=$(cd ../one && git rev-parse refs/heads/main) &&
		test_must_fail git fetch ../one/.git refs/heads/*:refs/remotes/one/* ^$main_in_one
	)
'

test_expect_failure "fetch with negative pattern refspec (grit: glob refspec expansion)" '
	echo >file updated by origin once more &&
	git commit -a -m "updated by origin once more" &&
	(
		cd three &&
		alternate_in_one=$(cd ../one && git rev-parse refs/heads/alternate) &&
		echo $alternate_in_one >expect &&
		git fetch ../one/.git refs/heads/*:refs/remotes/one/* ^refs/heads/m* &&
		cut -f -1 .git/FETCH_HEAD >actual &&
		test_cmp expect actual
	)
'

test_expect_failure "fetch with negative pattern refspec does not expand prefix (grit: glob refspec expansion)" '
	echo >file updated by origin another time &&
	git commit -a -m "updated by origin another time" &&
	(
		cd three &&
		alternate_in_one=$(cd ../one && git rev-parse refs/heads/alternate) &&
		main_in_one=$(cd ../one && git rev-parse refs/heads/main) &&
		echo $alternate_in_one >expect &&
		echo $main_in_one >>expect &&
		git fetch ../one/.git refs/heads/*:refs/remotes/one/* ^main &&
		cut -f -1 .git/FETCH_HEAD >actual &&
		test_cmp expect actual
	)
'

test_expect_failure "fetch with negative refspec avoids duplicate conflict (grit: negative refspec not supported)" '
	(
		cd one &&
		git branch dups/a &&
		git branch dups/b &&
		git branch dups/c &&
		git branch other/a &&
		git rev-parse --verify refs/heads/other/a >../expect &&
		git rev-parse --verify refs/heads/dups/b >>../expect &&
		git rev-parse --verify refs/heads/dups/c >>../expect
	) &&
	(
		cd three &&
		git fetch ../one/.git ^refs/heads/dups/a refs/heads/dups/*:refs/dups/* refs/heads/other/a:refs/dups/a &&
		git rev-parse --verify refs/dups/a >../actual &&
		git rev-parse --verify refs/dups/b >>../actual &&
		git rev-parse --verify refs/dups/c >>../actual
	) &&
	test_cmp expect actual
'

test_expect_failure "push --prune with negative refspec (grit: push glob refspec not supported)" '
	(
		cd two &&
		git branch prune/a &&
		git branch prune/b &&
		git branch prune/c &&
		git push ../three refs/heads/prune/* &&
		git branch -d prune/a &&
		git branch -d prune/b &&
		git push --prune ../three refs/heads/prune/* ^refs/heads/prune/b
	) &&
	(
		cd three &&
		test_write_lines b c >expect &&
		git for-each-ref --format="%(refname:lstrip=3)" refs/heads/prune/ >actual &&
		test_cmp expect actual
	)
'

test_expect_failure "push --prune with negative refspec apply to the destination (grit: push glob refspec not supported)" '
	(
		cd two &&
		git branch ours/a &&
		git branch ours/b &&
		git branch ours/c &&
		git push ../three refs/heads/ours/*:refs/heads/theirs/* &&
		git branch -d ours/a &&
		git branch -d ours/b &&
		git push --prune ../three refs/heads/ours/*:refs/heads/theirs/* ^refs/heads/theirs/b
	) &&
	(
		cd three &&
		test_write_lines b c >expect &&
		git for-each-ref --format="%(refname:lstrip=3)" refs/heads/theirs/ >actual &&
		test_cmp expect actual
	)
'

test_expect_failure "fetch --prune with negative refspec (grit: fetch glob refspec not supported)" '
	(
		cd two &&
		git branch fetch/a &&
		git branch fetch/b &&
		git branch fetch/c
	) &&
	(
		cd three &&
		git fetch ../two/.git refs/heads/fetch/*:refs/heads/copied/*
	) &&
	(
		cd two &&
		git branch -d fetch/a &&
		git branch -d fetch/b
	) &&
	(
		cd three &&
		test_write_lines b c >expect &&
		git fetch -v ../two/.git --prune refs/heads/fetch/*:refs/heads/copied/* ^refs/heads/fetch/b &&
		git for-each-ref --format="%(refname:lstrip=3)" refs/heads/copied/ >actual &&
		test_cmp expect actual
	)
'

test_expect_failure "push with matching : and negative refspec (grit: matching push and push -v not supported)" '
	test_when_finished "git -C two config --unset-all remote.one.push 2>/dev/null; true" &&

	git -C two config --add remote.one.push : &&

	test_must_fail git -C two push one &&

	current=$(git symbolic-ref HEAD) &&

	git -C two config --add remote.one.push "^$current" &&

	git -C two push -v one
'

test_expect_failure "push with matching +: and negative refspec (grit: matching push and push -v not supported)" '
	test_when_finished "git -C two config --unset-all remote.one.push 2>/dev/null; true" &&

	git -C two config --add remote.one.push +: &&

	test_must_fail git -C two push one &&

	current=$(git symbolic-ref HEAD) &&

	git -C two config --add remote.one.push "^$current" &&

	git -C two push -v one
'

test_expect_failure '--prefetch correctly modifies refspecs (grit: --prefetch not supported)' '
	git -C one config --unset-all remote.origin.fetch &&
	git -C one config --add remote.origin.fetch ^refs/heads/bogus/ignore &&
	git -C one config --add remote.origin.fetch "refs/tags/*:refs/tags/*" &&
	git -C one config --add remote.origin.fetch "refs/heads/bogus/*:bogus/*" &&

	git tag -a -m never never-fetch-tag HEAD &&

	git branch bogus/fetched HEAD~1 &&
	git branch bogus/ignore HEAD &&

	git -C one fetch --prefetch --no-tags &&
	test_must_fail git -C one rev-parse never-fetch-tag &&
	git -C one rev-parse refs/prefetch/bogus/fetched &&
	test_must_fail git -C one rev-parse refs/prefetch/bogus/ignore &&

	git -C one config --unset-all remote.origin.fetch &&
	git -C one config --add remote.origin.fetch "refs/tags/*:refs/tags/*" &&

	git -C one fetch --prefetch --no-tags &&
	test_must_fail git -C one rev-parse never-fetch-tag &&

	git -C one rev-parse refs/prefetch/bogus/fetched &&
	test_must_fail git -C one rev-parse refs/prefetch/bogus/ignore
'

test_expect_failure '--prefetch succeeds when refspec becomes empty (grit: --prefetch not supported)' '
	git checkout bogus/fetched &&
	test_commit extra &&

	git -C one config --unset-all remote.origin.fetch &&
	git -C one config --unset branch.main.remote &&
	git -C one config remote.origin.fetch "+refs/tags/extra" &&
	git -C one config remote.origin.skipfetchall true &&
	git -C one config remote.origin.tagopt "--no-tags" &&

	git -C one fetch --prefetch
'

test_expect_failure '--prefetch succeeds with empty command line refspec (grit: --prefetch not supported)' '
	git -C one fetch --prefetch origin +refs/tags/extra
'

test_done
