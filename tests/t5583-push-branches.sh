#!/bin/sh
# Ported from git/t/t5583-push-branches.sh
# Tests --all and --branches consistency for push

test_description='check the consistency of behavior of --all and --branches'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup bare remote' '
	git init -q &&
	git init --bare remote-1 &&
	git -C remote-1 config gc.auto 0 &&
	test_commit one &&
	git push ./remote-1 main
'

test_expect_success 'setup different types of references' '
	git branch branch-1 &&
	git branch branch-2 &&
	git tag -a -m "annotated" annotated-1 HEAD &&
	git tag -a -m "annotated" annotated-2 HEAD
'

# grit does not support --all for push
test_expect_failure '--all pushes all branches' '
	git push ./remote-1 --all &&
	commit=$(git rev-parse HEAD) &&
	cat >expect <<-EOF &&
	$commit refs/heads/branch-1
	$commit refs/heads/branch-2
	$commit refs/heads/main
	EOF
	git -C remote-1 show-ref --branches >actual &&
	test_cmp expect actual
'

# grit does not support --branches for push
test_expect_failure '--branches pushes all branches' '
	git init --bare remote-2 &&
	git push ./remote-2 main &&
	git push ./remote-2 --branches &&
	commit=$(git rev-parse HEAD) &&
	cat >expect <<-EOF &&
	$commit refs/heads/branch-1
	$commit refs/heads/branch-2
	$commit refs/heads/main
	EOF
	git -C remote-2 show-ref --branches >actual &&
	test_cmp expect actual
'

# grit does not support --all for push
test_expect_failure '--all or --branches can not be combined with refspecs' '
	test_must_fail git push ./remote-1 --all main 2>actual &&
	grep "be combined with refspecs" actual
'

# grit does not support --all for push
test_expect_failure '--all or --branches can not be combined with --mirror' '
	test_must_fail git push ./remote-1 --all --mirror 2>actual &&
	grep "cannot be used together" actual
'

# grit does not support --all for push
test_expect_failure '--all or --branches can not be combined with --tags' '
	test_must_fail git push ./remote-1 --all --tags 2>actual &&
	grep "cannot be used together" actual
'

# grit does not support --all for push
test_expect_failure '--all or --branches can not be combined with --delete' '
	test_must_fail git push ./remote-1 --all --delete 2>actual &&
	grep "cannot be used together" actual
'

test_expect_success 'push individual branches works' '
	git init --bare remote-3 &&
	git push ./remote-3 main branch-1 branch-2 &&
	commit=$(git rev-parse HEAD) &&
	git -C remote-3 rev-parse main >actual &&
	echo $commit >expect &&
	test_cmp expect actual &&
	git -C remote-3 rev-parse branch-1 >actual &&
	test_cmp expect actual &&
	git -C remote-3 rev-parse branch-2 >actual &&
	test_cmp expect actual
'

test_done
