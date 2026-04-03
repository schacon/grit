#!/bin/sh

test_description='check random commands outside repo'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'set up a non-repo directory' '
	mkdir -p non-repo
'

test_expect_success 'hash-object outside repository' '
	echo "test content" >sample &&
	tmpdir=$(mktemp -d) &&
	cp sample "$tmpdir/" &&
	(
		cd "$tmpdir" &&
		GIT_CEILING_DIRECTORIES="$tmpdir" &&
		export GIT_CEILING_DIRECTORIES &&
		git hash-object --stdin <sample >actual
	) &&
	git hash-object --stdin <sample >expect &&
	test_cmp expect "$tmpdir/actual" &&
	rm -rf "$tmpdir"
'

test_expect_success 'check-ref-format outside repository' '
	tmpdir=$(mktemp -d) &&
	(
		cd "$tmpdir" &&
		GIT_CEILING_DIRECTORIES="$tmpdir" &&
		export GIT_CEILING_DIRECTORIES &&
		git check-ref-format --branch refs/heads/main >actual
	) &&
	echo refs/heads/main >expect &&
	test_cmp expect "$tmpdir/actual" &&
	rm -rf "$tmpdir"
'

test_expect_success 'stripspace outside repository' '
	tmpdir=$(mktemp -d) &&
	(
		cd "$tmpdir" &&
		GIT_CEILING_DIRECTORIES="$tmpdir" &&
		export GIT_CEILING_DIRECTORIES &&
		echo "  hello  " | git stripspace >actual
	) &&
	echo "  hello" >expect &&
	test_cmp expect "$tmpdir/actual" &&
	rm -rf "$tmpdir"
'

test_expect_success 'apply a patch outside repository' '
	git init patch-test &&
	(
		cd patch-test &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		printf "one\ntwo\nthree\nfour\n" >nums &&
		git add nums &&
		cp nums nums.old &&
		printf "one\ntwo\nthree\nfour\nfive\n" >nums &&
		git diff >sample.patch
	) &&
	patch_abs="$PWD/patch-test/sample.patch" &&
	tmpdir=$(mktemp -d) &&
	cp patch-test/nums.old "$tmpdir/nums" &&
	(
		cd "$tmpdir" &&
		GIT_CEILING_DIRECTORIES="$tmpdir" &&
		export GIT_CEILING_DIRECTORIES &&
		git apply "$patch_abs"
	) &&
	test_cmp patch-test/nums "$tmpdir/nums" &&
	rm -rf "$tmpdir"
'

test_expect_success 'diff --no-index outside repository' '
	echo one >one &&
	echo two >two &&
	tmpdir=$(mktemp -d) &&
	cp one two "$tmpdir/" &&
	(
		cd "$tmpdir" &&
		GIT_CEILING_DIRECTORIES="$tmpdir" &&
		export GIT_CEILING_DIRECTORIES &&
		test_must_fail git diff --no-index one two >actual
	) &&
	test -s "$tmpdir/actual" &&
	rm -rf "$tmpdir"
'

test_done
