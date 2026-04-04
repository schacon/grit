#!/bin/sh
# Ported from git/t/t5533-push-cas.sh

test_description='compare & swap push force/delete safety'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

setup_srcdst_basic () {
	rm -fr src dst &&
	git clone --no-local . src &&
	git clone --no-local src dst &&
	(
		cd src && git checkout HEAD^0
	)
}

# For tests with "--force-if-includes".
setup_src_dup_dst () {
	rm -fr src dup dst &&
	git init --bare dst &&
	git clone --no-local dst src &&
	git clone --no-local dst dup
	(
		cd src &&
		test_commit A &&
		test_commit B &&
		test_commit C &&
		git push origin
	) &&
	(
		cd dup &&
		git fetch &&
		git merge origin/main &&
		git checkout -b branch main~2 &&
		test_commit D &&
		test_commit E &&
		git push origin --all
	) &&
	(
		cd src &&
		git checkout main &&
		git fetch --all &&
		git branch branch --track origin/branch &&
		git rebase origin/main
	) &&
	(
		cd dup &&
		git checkout main &&
		test_commit F &&
		test_commit G &&
		git checkout branch &&
		test_commit H &&
		git push origin --all
	)
}

test_expect_success setup '
	# create template repository
	git init &&
	test_commit A &&
	test_commit B &&
	test_commit C
'

test_expect_failure 'push to update (protected) (grit: --force-with-lease=ref:expected not fully supported)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		test_commit D &&
		test_must_fail git push --force-with-lease=main:main origin main 2>err &&
		grep "stale info" err
	) &&
	git ls-remote . refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_failure 'push to update (protected, forced) (grit: --force-with-lease=ref:expected not fully supported)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		test_commit D &&
		git push --force --force-with-lease=main:main origin main 2>err &&
		grep "forced update" err
	) &&
	git ls-remote dst refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'push to update (protected, tracking)' '
	setup_srcdst_basic &&
	(
		cd src &&
		git checkout main &&
		test_commit D &&
		git checkout HEAD^0
	) &&
	git ls-remote src refs/heads/main >expect &&
	(
		cd dst &&
		test_commit E &&
		git ls-remote . refs/remotes/origin/main >expect &&
		test_must_fail git push --force-with-lease=main origin main &&
		git ls-remote . refs/remotes/origin/main >actual &&
		test_cmp expect actual
	) &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_failure 'push to update (protected, tracking, forced) (grit: force + force-with-lease interaction)' '
	setup_srcdst_basic &&
	(
		cd src &&
		git checkout main &&
		test_commit D &&
		git checkout HEAD^0
	) &&
	(
		cd dst &&
		test_commit E &&
		git ls-remote . refs/remotes/origin/main >expect &&
		git push --force --force-with-lease=main origin main
	) &&
	git ls-remote dst refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_failure 'push to update (allowed) (grit: --force-with-lease=ref:expected not fully supported)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		test_commit D &&
		git push --force-with-lease=main:main^ origin main
	) &&
	git ls-remote dst refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_failure 'push to update (allowed, tracking) (grit: force-with-lease tracking behavior)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		test_commit D &&
		git push --force-with-lease=main origin main 2>err &&
		! grep "forced update" err
	) &&
	git ls-remote dst refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_failure 'push to update (allowed even though no-ff) (grit: force-with-lease non-ff behavior)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		git reset --hard HEAD^ &&
		test_commit D &&
		git push --force-with-lease=main origin main 2>err &&
		grep "forced update" err
	) &&
	git ls-remote dst refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'push to delete (protected)' '
	setup_srcdst_basic &&
	git ls-remote src refs/heads/main >expect &&
	(
		cd dst &&
		test_must_fail git push --force-with-lease=main:main^ origin :main
	) &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_failure 'push to delete (protected, forced) (grit: force + force-with-lease delete interaction)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		git push --force --force-with-lease=main:main^ origin :main
	) &&
	git ls-remote src refs/heads/main >actual &&
	test_must_be_empty actual
'

test_expect_failure 'push to delete (allowed) (grit: force-with-lease delete behavior)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		git push --force-with-lease=main origin :main 2>err &&
		grep deleted err
	) &&
	git ls-remote src refs/heads/main >actual &&
	test_must_be_empty actual
'

test_expect_failure 'cover everything with default force-with-lease (protected) (grit: default force-with-lease behavior)' '
	setup_srcdst_basic &&
	(
		cd src &&
		git branch nain main^
	) &&
	git ls-remote src refs/heads/\* >expect &&
	(
		cd dst &&
		test_must_fail git push --force-with-lease origin main main:nain
	) &&
	git ls-remote src refs/heads/\* >actual &&
	test_cmp expect actual
'

test_expect_success 'cover everything with default force-with-lease (allowed)' '
	setup_srcdst_basic &&
	(
		cd src &&
		git branch nain main^
	) &&
	(
		cd dst &&
		git fetch &&
		git push --force-with-lease origin main main:nain
	) &&
	git ls-remote dst refs/heads/main |
	sed -e "s/main/nain/" >expect &&
	git ls-remote src refs/heads/nain >actual &&
	test_cmp expect actual
'

test_expect_failure 'new branch covered by force-with-lease (grit: force-with-lease for new branches)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		git branch branch main &&
		git push --force-with-lease=branch origin branch
	) &&
	git ls-remote dst refs/heads/branch >expect &&
	git ls-remote src refs/heads/branch >actual &&
	test_cmp expect actual
'

test_expect_failure 'new branch covered by force-with-lease (explicit) (grit: force-with-lease=ref: syntax)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		git branch branch main &&
		git push --force-with-lease=branch: origin branch
	) &&
	git ls-remote dst refs/heads/branch >expect &&
	git ls-remote src refs/heads/branch >actual &&
	test_cmp expect actual
'

test_expect_success 'new branch already exists' '
	setup_srcdst_basic &&
	(
		cd src &&
		git checkout -b branch main &&
		test_commit F
	) &&
	(
		cd dst &&
		git branch branch main &&
		test_must_fail git push --force-with-lease=branch: origin branch
	)
'

test_expect_failure 'background updates of REMOTE can be mitigated with a non-updated REMOTE-push (grit: clone empty bare repo)' '
	rm -rf src dst &&
	git init --bare src.bare &&
	test_when_finished "rm -rf src.bare" &&
	git clone --no-local src.bare dst &&
	test_when_finished "rm -rf dst" &&
	(
		cd dst &&
		test_commit G &&
		git remote add origin-push ../src.bare &&
		git push origin-push main:main
	) &&
	git clone --no-local src.bare dst2 &&
	test_when_finished "rm -rf dst2" &&
	(
		cd dst2 &&
		test_commit H &&
		git push
	) &&
	(
		cd dst &&
		test_commit I &&
		git fetch origin &&
		test_must_fail git push --force-with-lease origin-push &&
		git fetch origin-push &&
		git push --force-with-lease origin-push
	)
'

test_expect_failure 'background updates to remote can be mitigated with "--force-if-includes" (grit: missing merge/rebase/--force-if-includes)' '
	setup_src_dup_dst &&
	test_when_finished "rm -fr dst src dup" &&
	git ls-remote dst refs/heads/main >expect.main &&
	git ls-remote dst refs/heads/branch >expect.branch &&
	(
		cd src &&
		git checkout branch &&
		test_commit I &&
		git checkout main &&
		test_commit J &&
		git fetch --all &&
		test_must_fail git push --force-with-lease --force-if-includes --all
	) &&
	git ls-remote dst refs/heads/main >actual.main &&
	git ls-remote dst refs/heads/branch >actual.branch &&
	test_cmp expect.main actual.main &&
	test_cmp expect.branch actual.branch
'

test_expect_failure 'background updates to remote can be mitigated with "push.useForceIfIncludes" (grit: missing merge/rebase/--force-if-includes)' '
	setup_src_dup_dst &&
	test_when_finished "rm -fr dst src dup" &&
	git ls-remote dst refs/heads/main >expect.main &&
	(
		cd src &&
		git checkout branch &&
		test_commit I &&
		git checkout main &&
		test_commit J &&
		git fetch --all &&
		git config --local push.useForceIfIncludes true &&
		test_must_fail git push --force-with-lease=main origin main
	) &&
	git ls-remote dst refs/heads/main >actual.main &&
	test_cmp expect.main actual.main
'

test_expect_failure '"--force-if-includes" should be disabled for --force-with-lease="<refname>:<expect>" (grit: missing --force-if-includes)' '
	setup_src_dup_dst &&
	test_when_finished "rm -fr dst src dup" &&
	git ls-remote dst refs/heads/main >expect.main &&
	(
		cd src &&
		git checkout branch &&
		test_commit I &&
		git checkout main &&
		test_commit J &&
		remote_head="$(git rev-parse refs/remotes/origin/main)" &&
		git fetch --all &&
		test_must_fail git push --force-if-includes --force-with-lease="main:$remote_head" 2>err &&
		grep "stale info" err
	) &&
	git ls-remote dst refs/heads/main >actual.main &&
	test_cmp expect.main actual.main
'

test_expect_failure '"--force-if-includes" should allow forced update after a rebase ("pull --rebase") (grit: missing rebase)' '
	setup_src_dup_dst &&
	test_when_finished "rm -fr dst src dup" &&
	(
		cd src &&
		git checkout branch &&
		test_commit I &&
		git checkout main &&
		test_commit J &&
		git pull --rebase origin main &&
		git push --force-if-includes --force-with-lease="main"
	)
'

test_expect_failure '"--force-if-includes" should allow forced update after a rebase ("pull --rebase", local rebase) (grit: missing rebase)' '
	setup_src_dup_dst &&
	test_when_finished "rm -fr dst src dup" &&
	(
		cd src &&
		git checkout branch &&
		test_commit I &&
		git checkout main &&
		test_commit J &&
		git pull --rebase origin main &&
		git rebase --onto HEAD~4 HEAD~1 &&
		git push --force-if-includes --force-with-lease="main"
	)
'

test_expect_failure '"--force-if-includes" should allow deletes (grit: missing --force-if-includes/rebase)' '
	setup_src_dup_dst &&
	test_when_finished "rm -fr dst src dup" &&
	(
		cd src &&
		git checkout branch &&
		git pull --rebase origin branch &&
		git push --force-if-includes --force-with-lease="branch" origin :branch
	)
'

test_done
