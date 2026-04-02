#!/bin/sh
# Test rev-parse with parent notation (^, ~) and related options.

test_description='rev-parse with parent notation (^, ~)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ──────────────────────────────────────────────────────────────────────

test_expect_success 'setup linear history with 5 commits' '
	grit init repo &&
	cd repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	echo c1 >f.txt && grit add f.txt && grit commit -m "c1" &&
	grit rev-parse HEAD >../c1 &&
	echo c2 >f.txt && grit add f.txt && grit commit -m "c2" &&
	grit rev-parse HEAD >../c2 &&
	echo c3 >f.txt && grit add f.txt && grit commit -m "c3" &&
	grit rev-parse HEAD >../c3 &&
	echo c4 >f.txt && grit add f.txt && grit commit -m "c4" &&
	grit rev-parse HEAD >../c4 &&
	echo c5 >f.txt && grit add f.txt && grit commit -m "c5" &&
	grit rev-parse HEAD >../c5
'

# ── Basic ^ (caret) ──────────────────────────────────────────────────────────

test_expect_success 'HEAD^ resolves to parent' '
	cd repo &&
	result=$(grit rev-parse HEAD^) &&
	expected=$(cat ../c4) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD^^ resolves to grandparent' '
	cd repo &&
	result=$(grit rev-parse HEAD^^) &&
	expected=$(cat ../c3) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD^^^ resolves to great-grandparent' '
	cd repo &&
	result=$(grit rev-parse HEAD^^^) &&
	expected=$(cat ../c2) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD^^^^ resolves to root commit' '
	cd repo &&
	result=$(grit rev-parse HEAD^^^^) &&
	expected=$(cat ../c1) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD^0 resolves to HEAD itself' '
	cd repo &&
	result=$(grit rev-parse HEAD^0) &&
	expected=$(grit rev-parse HEAD) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD^1 is same as HEAD^' '
	cd repo &&
	r1=$(grit rev-parse HEAD^1) &&
	r2=$(grit rev-parse HEAD^) &&
	test "$r1" = "$r2"
'

# ── Tilde (~) ────────────────────────────────────────────────────────────────

test_expect_success 'HEAD~1 resolves to parent' '
	cd repo &&
	result=$(grit rev-parse HEAD~1) &&
	expected=$(cat ../c4) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD~2 resolves to grandparent' '
	cd repo &&
	result=$(grit rev-parse HEAD~2) &&
	expected=$(cat ../c3) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD~3 resolves to great-grandparent' '
	cd repo &&
	result=$(grit rev-parse HEAD~3) &&
	expected=$(cat ../c2) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD~4 resolves to root commit' '
	cd repo &&
	result=$(grit rev-parse HEAD~4) &&
	expected=$(cat ../c1) &&
	test "$result" = "$expected"
'

test_expect_success 'HEAD~1 equals HEAD^' '
	cd repo &&
	r1=$(grit rev-parse HEAD~1) &&
	r2=$(grit rev-parse HEAD^) &&
	test "$r1" = "$r2"
'

test_expect_success 'HEAD~2 equals HEAD^^' '
	cd repo &&
	r1=$(grit rev-parse HEAD~2) &&
	r2=$(grit rev-parse HEAD^^) &&
	test "$r1" = "$r2"
'

# ── Error cases ──────────────────────────────────────────────────────────────

test_expect_success 'parent of root commit fails' '
	cd repo &&
	root=$(cat ../c1) &&
	test_must_fail grit rev-parse "${root}^" 2>err
'

test_expect_success 'HEAD~5 on 5-commit history fails' '
	cd repo &&
	test_must_fail grit rev-parse HEAD~5 2>err
'

test_expect_success 'HEAD^2 on linear history fails' '
	cd repo &&
	test_must_fail grit rev-parse HEAD^2 2>err
'

test_expect_success 'nonexistent ref fails with --verify' '
	cd repo &&
	test_must_fail grit rev-parse --verify nonexistent 2>err
'

# ── Tags with parent notation ────────────────────────────────────────────────

test_expect_success 'setup tags' '
	cd repo &&
	grit tag v1 $(cat ../c3) &&
	grit tag v2 $(cat ../c5)
'

test_expect_success 'tag^ resolves to parent of tagged commit' '
	cd repo &&
	result=$(grit rev-parse v1^) &&
	expected=$(cat ../c2) &&
	test "$result" = "$expected"
'

test_expect_success 'tag~2 resolves correctly' '
	cd repo &&
	result=$(grit rev-parse v1~2) &&
	expected=$(cat ../c1) &&
	test "$result" = "$expected"
'

test_expect_success 'tag at HEAD: v2^ equals HEAD^' '
	cd repo &&
	r1=$(grit rev-parse v2^) &&
	r2=$(grit rev-parse HEAD^) &&
	test "$r1" = "$r2"
'

# ── SHA with parent notation ────────────────────────────────────────────────

test_expect_success 'full SHA^ resolves correctly' '
	cd repo &&
	sha=$(cat ../c5) &&
	result=$(grit rev-parse "${sha}^") &&
	expected=$(cat ../c4) &&
	test "$result" = "$expected"
'

test_expect_success 'full SHA~2 resolves correctly' '
	cd repo &&
	sha=$(cat ../c5) &&
	result=$(grit rev-parse "${sha}~2") &&
	expected=$(cat ../c3) &&
	test "$result" = "$expected"
'

# ── Branches with parent notation ────────────────────────────────────────────

test_expect_success 'master^ resolves to parent of branch tip' '
	cd repo &&
	result=$(grit rev-parse master^) &&
	expected=$(cat ../c4) &&
	test "$result" = "$expected"
'

test_expect_success 'master~3 resolves correctly' '
	cd repo &&
	result=$(grit rev-parse master~3) &&
	expected=$(cat ../c2) &&
	test "$result" = "$expected"
'

# ── --verify ─────────────────────────────────────────────────────────────────

test_expect_success '--verify HEAD resolves' '
	cd repo &&
	result=$(grit rev-parse --verify HEAD) &&
	expected=$(cat ../c5) &&
	test "$result" = "$expected"
'

test_expect_success '--verify with parent notation works' '
	cd repo &&
	result=$(grit rev-parse --verify HEAD^) &&
	expected=$(cat ../c4) &&
	test "$result" = "$expected"
'

# ── --short ──────────────────────────────────────────────────────────────────

test_expect_success '--short outputs abbreviated OID' '
	cd repo &&
	short=$(grit rev-parse --short HEAD) &&
	full=$(grit rev-parse HEAD) &&
	# short should be a prefix of full
	echo "$full" | grep "^${short}"
'

test_expect_success '--short HEAD^ works' '
	cd repo &&
	short=$(grit rev-parse --short HEAD^) &&
	full=$(grit rev-parse HEAD^) &&
	echo "$full" | grep "^${short}"
'

# ── --git-dir / --show-toplevel ──────────────────────────────────────────────

test_expect_success '--git-dir returns .git' '
	cd repo &&
	grit rev-parse --git-dir >out &&
	grep ".git" out
'

test_expect_success '--show-toplevel returns repo root' '
	cd repo &&
	grit rev-parse --show-toplevel >out &&
	test -s out
'

test_done
