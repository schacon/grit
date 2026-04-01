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

test_done
