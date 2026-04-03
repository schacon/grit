#!/bin/sh
#
# Ported from git/t/t1450-fsck.sh (small subset)

test_description='git fsck basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'setup' '
	test_commit A &&
	test_commit B
'

test_expect_success 'HEAD is part of refs, valid objects appear valid' '
	git fsck >actual 2>&1 &&
	test_must_be_empty actual
'

test_expect_success 'fsck notices missing blob' '
	blob=$(echo blob | git hash-object -w --stdin) &&
	tree=$(git rev-parse HEAD^{tree}) &&
	echo "100644 blob $blob	foobar" >tree_in &&
	newtree=$(git mktree <tree_in) &&
	parent=$(git rev-parse HEAD) &&
	test_tick &&
	commit=$(echo "broken" | git commit-tree $newtree -p $parent) &&
	git update-ref refs/heads/broken $commit &&
	rm .git/objects/$(echo $blob | sed "s|^..|&/|") &&
	test_must_fail git fsck 2>err &&
	grep "$blob" err
'

test_done
