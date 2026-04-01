#!/bin/sh
# Tests for grit ls-remote (local path transport only).

test_description='ls-remote with local repository path'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Two stable fake OIDs used throughout these tests.
A=1111111111111111111111111111111111111111
B=2222222222222222222222222222222222222222

test_expect_success 'setup: create a local remote repository' '
	grit init remote &&
	cd remote &&
	grit update-ref refs/heads/main "$A" &&
	grit update-ref refs/heads/topic "$B" &&
	grit update-ref refs/tags/v1.0 "$A" &&
	grit symbolic-ref HEAD refs/heads/main &&
	cd ..
'

test_expect_success 'ls-remote lists HEAD then refs in sorted order' '
	printf "%s\tHEAD\n" "$A" >expect &&
	printf "%s\trefs/heads/main\n" "$A" >>expect &&
	printf "%s\trefs/heads/topic\n" "$B" >>expect &&
	printf "%s\trefs/tags/v1.0\n" "$A" >>expect &&
	grit ls-remote remote >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-remote --heads shows only branches' '
	printf "%s\trefs/heads/main\n" "$A" >expect &&
	printf "%s\trefs/heads/topic\n" "$B" >>expect &&
	grit ls-remote --heads remote >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-remote --tags shows only tags' '
	printf "%s\trefs/tags/v1.0\n" "$A" >expect &&
	grit ls-remote --tags remote >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-remote --refs excludes HEAD' '
	printf "%s\trefs/heads/main\n" "$A" >expect &&
	printf "%s\trefs/heads/topic\n" "$B" >>expect &&
	printf "%s\trefs/tags/v1.0\n" "$A" >>expect &&
	grit ls-remote --refs remote >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-remote --symref shows symbolic ref line before HEAD' '
	printf "ref: refs/heads/main\tHEAD\n" >expect &&
	printf "%s\tHEAD\n" "$A" >>expect &&
	printf "%s\trefs/heads/main\n" "$A" >>expect &&
	printf "%s\trefs/heads/topic\n" "$B" >>expect &&
	printf "%s\trefs/tags/v1.0\n" "$A" >>expect &&
	grit ls-remote --symref remote >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-remote -q produces no output and exits 0' '
	grit ls-remote -q remote >actual &&
	test_must_fail test -s actual
'

test_expect_success 'ls-remote with pattern filters to matching refs' '
	printf "%s\trefs/heads/main\n" "$A" >expect &&
	grit ls-remote remote main >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-remote reads packed-refs' '
	C=3333333333333333333333333333333333333333 &&
	grit init packed-remote &&
	GIT_DIR=packed-remote/.git grit update-ref refs/heads/main "$A" &&
	GIT_DIR=packed-remote/.git grit symbolic-ref HEAD refs/heads/main &&
	printf "%s refs/heads/packed-branch\n" "$C" \
		>packed-remote/.git/packed-refs &&
	printf "%s\tHEAD\n" "$A" >expect &&
	printf "%s\trefs/heads/main\n" "$A" >>expect &&
	printf "%s\trefs/heads/packed-branch\n" "$C" >>expect &&
	grit ls-remote packed-remote >actual &&
	test_cmp expect actual
'

test_done
