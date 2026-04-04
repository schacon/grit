#!/bin/sh
# Ported from git/t/t5532-fetch-proxy.sh
# Tests fetching via git:// using core.gitproxy
# Grit does not support the git:// protocol or core.gitproxy

test_description='fetching via git:// using core.gitproxy'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# The upstream test requires Perl and the git:// protocol with core.gitproxy.
# Grit only supports local and file:// transports, so these tests are all
# expected to fail.

test_expect_success 'setup remote repo' '
	git init -q &&
	git init remote &&
	(cd remote &&
	 echo content >file &&
	 git add file &&
	 git commit -m one
	)
'

# grit does not support git:// protocol
test_expect_success 'fetch through proxy works' '
	git remote add fake git://example.com/remote &&
	git config core.gitproxy ./proxy &&
	git fetch fake &&
	echo one >expect &&
	git log -1 --format=%s FETCH_HEAD >actual &&
	test_cmp expect actual
'

# grit rejects git:// URLs outright (no proxy needed)
test_expect_success 'funny hostnames are rejected before running proxy' '
	test_must_fail git fetch git://-remote/repo.git 2>stderr &&
	! grep "proxying for" stderr
'

test_done
