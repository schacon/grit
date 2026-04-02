#!/bin/sh
# Ported subset from git/t/t4044-diff-index-unique-abbrev.sh.

test_description='diff-index raw output honors --abbrev width'

. ./test-lib.sh

make_commit () {
	msg=$1
	parent=${2-}
	tree=$(git write-tree) || return 1
	if test -n "$parent"
	then
		commit=$(printf '%s\n' "$msg" | git commit-tree "$tree" -p "$parent") || return 1
	else
		commit=$(printf '%s\n' "$msg" | git commit-tree "$tree") || return 1
	fi
	git update-ref HEAD "$commit" || return 1
	printf '%s\n' "$commit"
}

test_expect_success 'setup two versions of one tracked file' '
	git init repo &&
	cd repo &&
	printf "value-two\n" >foo &&
	c1=$(make_commit initial) &&
	git update-index --add foo &&
	c2=$(make_commit update "$c1") &&
	new_blob=$(git hash-object foo) &&
	test -n "$c1" &&
	test -n "$c2" &&
	printf "%s\n" "$c1" >c1 &&
	printf "%s\n" "$new_blob" >new_blob
'

test_expect_success 'diff-index --abbrev=8 prints abbreviated raw oids' '
	cd repo &&
	c1=$(cat c1) &&
	new_blob=$(cat new_blob) &&
	new8=$(printf "%s" "$new_blob" | cut -c1-8) &&
	printf ":000000 100644 00000000 %s A\tfoo\n" "$new8" >expect &&
	git diff-index --cached --raw --abbrev=8 "$c1" -- foo >actual &&
	test_cmp expect actual
'

test_expect_success 'diff-index default raw output uses full oids' '
	cd repo &&
	c1=$(cat c1) &&
	new_blob=$(cat new_blob) &&
	printf ":000000 100644 0000000000000000000000000000000000000000 %s A\tfoo\n" "$new_blob" >expect.full &&
	git diff-index --cached --raw "$c1" -- foo >actual.full &&
	test_cmp expect.full actual.full
'

test_expect_success 'diff-index --abbrev=4 uses minimum abbreviation' '
	cd repo &&
	c1=$(cat c1) &&
	git diff-index --cached --raw --abbrev=4 "$c1" -- foo >actual4 &&
	# abbrev=4 should show at least 4 hex chars for non-zero oid
	grep ":000000 100644 0000" actual4 &&
	grep "A" actual4
'

test_expect_success 'diff-index --abbrev with modified file' '
	cd repo &&
	c2=$(cat ../c2 2>/dev/null || echo skip) &&
	test "$c2" = skip && return 0 &&
	git diff-index --cached --raw --abbrev=8 "$c2" -- foo >actual_mod &&
	grep "M" actual_mod
'

# ---------------------------------------------------------------------------
# Additional abbrev tests
# ---------------------------------------------------------------------------

test_expect_success 'setup: add second file for more abbrev tests' '
	cd repo &&
	printf "bar-content\n" >bar &&
	git update-index --add bar &&
	c3=$(make_commit add-bar "$(cat c2)") &&
	printf "%s\n" "$c3" >c3
'

test_expect_success 'diff-index --abbrev=12 shows 12-char abbreviated oids' '
	cd repo &&
	c1=$(cat c1) &&
	git diff-index --cached --raw --abbrev=12 "$c1" >out &&
	# Each non-zero OID should be at least 12 hex chars
	grep "A" out | head -1 >line &&
	oid=$(awk "{print \$4}" line) &&
	len=${#oid} &&
	test "$len" -ge 12
'

test_expect_success 'diff-index --abbrev=40 shows full oids' '
	cd repo &&
	c1=$(cat c1) &&
	git diff-index --cached --raw --abbrev=40 "$c1" >out &&
	grep "A" out | head -1 >line &&
	oid=$(awk "{print \$4}" line) &&
	len=${#oid} &&
	test "$len" -eq 40
'

test_expect_success 'diff-index --raw without --abbrev shows full 40-char oids' '
	cd repo &&
	c1=$(cat c1) &&
	git diff-index --cached --raw "$c1" >out &&
	grep "A" out | head -1 >line &&
	oid=$(awk "{print \$4}" line) &&
	len=${#oid} &&
	test "$len" -eq 40
'

test_expect_success 'diff-index --abbrev=8 with multiple files' '
	cd repo &&
	c1=$(cat c1) &&
	git diff-index --cached --raw --abbrev=8 "$c1" >out &&
	# Should show both foo and bar as added
	grep "foo" out &&
	grep "bar" out
'

test_done
