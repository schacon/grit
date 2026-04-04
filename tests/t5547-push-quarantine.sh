#!/bin/sh
# Ported from git/t/t5547-push-quarantine.sh
# Tests quarantine of objects during push

test_description='check quarantine of objects during push'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup and create picky dest repo' '
	git init -q &&
	git init --bare dest.git &&
	test_hook --setup -C dest.git pre-receive <<-\EOF
	while read old new ref; do
		test "$(git log -1 --format=%s $new)" = reject && exit 1
	done
	exit 0
	EOF
'

test_expect_success 'accepted objects work' '
	test_commit ok &&
	git push ./dest.git HEAD &&
	commit=$(git rev-parse HEAD) &&
	git --git-dir=dest.git cat-file commit $commit
'

# grit pre-receive hook support: the push should fail when hook rejects
test_expect_success 'rejected objects are not installed' '
	test_commit reject &&
	commit=$(git rev-parse HEAD) &&
	test_must_fail git push ./dest.git HEAD &&
	test_must_fail git --git-dir=dest.git cat-file commit $commit
'

# No quarantine files since grit doesn't use quarantine
test_expect_success 'rejected objects are removed' '
	echo "incoming-*" >expect &&
	(cd dest.git/objects && echo incoming-*) >actual &&
	test_cmp expect actual
'

test_expect_success 'push to repo path with path separator (colon)' '
	dd if=/dev/urandom bs=4096 count=1 2>/dev/null >file.bin &&
	git add file.bin &&
	git commit -m bin &&
	pathsep=":" &&
	git clone --bare . "xxx${pathsep}yyy.git" &&
	echo change >>file.bin &&
	git commit -am change &&
	# Note that we have to use the full path here, or it gets confused
	# with the ssh host:path syntax.
	git push "$(pwd)/xxx${pathsep}yyy.git" HEAD
'

# grit may not support quarantine + hook ref updates
test_expect_success 'updating a ref from quarantine is forbidden' '
	git init --bare update.git &&
	test_hook -C update.git pre-receive <<-\EOF &&
	read old new refname
	git update-ref refs/heads/unrelated $new
	exit 1
	EOF
	test_must_fail git push ./update.git HEAD &&
	git -C update.git fsck
'

test_done
