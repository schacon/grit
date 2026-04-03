#!/bin/sh
# Test --glob, --branches, --tags, --remotes for rev-list and rev-parse.

test_description='rev-list/rev-parse glob options (--glob, --branches, --tags, --remotes)'

. ./test-lib.sh

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

test_expect_success 'setup branches, tags, and remotes' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	test_commit base &&
	test_commit topic-one &&
	git branch topic/one &&
	test_commit topic-two &&
	git branch topic/two &&
	test_commit zzz &&
	git branch zzz &&
	git tag v1.0 base &&
	git tag v2.0 topic-one &&
	git tag release/candidate zzz &&
	mkdir -p .git/refs/remotes/origin &&
	git rev-parse topic/one >.git/refs/remotes/origin/one &&
	git rev-parse topic/two >.git/refs/remotes/origin/two
'

test_expect_failure 'rev-list --glob=refs/heads/topic/* lists matching branch commits' '
	cd repo &&
	git rev-list --glob="refs/heads/topic/*" >actual &&
	git rev-parse topic/one >expect &&
	git rev-parse topic/two >>expect &&
	sort actual >actual.sorted &&
	sort expect >expect.sorted &&
	test_cmp expect.sorted actual.sorted
'

test_expect_success 'rev-list --glob=refs/tags/* lists all tags' '
	cd repo &&
	git rev-list --glob="refs/tags/*" >actual &&
	test $(wc -l <actual | tr -d " ") -ge 3
'

test_expect_success 'rev-parse --branches=topic/* resolves topic branches' '
	cd repo &&
	git rev-parse --branches="topic/*" >actual &&
	git rev-parse topic/one >expect &&
	git rev-parse topic/two >>expect &&
	sort actual >actual.sorted &&
	sort expect >expect.sorted &&
	test_cmp expect.sorted actual.sorted
'

test_expect_success 'rev-parse --branches (no pattern) resolves all branches' '
	cd repo &&
	git rev-parse --branches >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" -ge 4
'

test_expect_success 'rev-parse --tags resolves all tags' '
	cd repo &&
	git rev-parse --tags >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" -ge 3
'

test_expect_success 'rev-parse --tags=v* resolves matching tags' '
	cd repo &&
	git rev-parse --tags="v*" >actual &&
	git rev-parse v1.0 >expect &&
	git rev-parse v2.0 >>expect &&
	sort actual >actual.sorted &&
	sort expect >expect.sorted &&
	test_cmp expect.sorted actual.sorted
'

test_expect_success 'rev-parse --remotes resolves all remote refs' '
	cd repo &&
	git rev-parse --remotes >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" -ge 2
'

test_expect_success 'rev-parse --remotes=origin/* resolves matching remotes' '
	cd repo &&
	git rev-parse --remotes="origin/*" >actual &&
	git rev-parse topic/one >expect &&
	git rev-parse topic/two >>expect &&
	sort actual >actual.sorted &&
	sort expect >expect.sorted &&
	test_cmp expect.sorted actual.sorted
'

test_expect_success 'rev-list --branches lists commits reachable from all branches' '
	cd repo &&
	git rev-list --branches >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" -ge 4
'

test_expect_success 'rev-list --tags lists commits reachable from all tags' '
	cd repo &&
	git rev-list --tags >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" -ge 1
'

test_expect_success 'rev-list --remotes lists commits reachable from remotes' '
	cd repo &&
	git rev-list --remotes >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" -ge 2
'

test_done
