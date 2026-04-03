#!/bin/sh

test_description='send-pack tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

create_ref () {
	tree=$(git write-tree) &&
	test_tick &&
	commit=$(echo "$1" | git commit-tree $tree) &&
	git update-ref "$1" $commit
}

clear_remote () {
	rm -rf remote.git &&
	git init --bare remote.git
}

verify_push () {
	git rev-parse "$1" >expect &&
	git --git-dir=remote.git rev-parse "${2:-$1}" >actual &&
	test_cmp expect actual
}

test_expect_success 'setup refs' '
	git init &&
	cat >refs <<-\EOF &&
	refs/heads/A
	refs/heads/C
	refs/tags/D
	refs/heads/B
	refs/tags/E
	EOF
	for i in $(cat refs); do
		create_ref $i || return 1
	done
'

test_expect_success 'refs on cmdline' '
	clear_remote &&
	git send-pack remote.git $(cat refs) &&
	for i in $(cat refs); do
		verify_push $i || return 1
	done
'

test_done
