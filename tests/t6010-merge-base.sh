#!/bin/sh
# Ported subset from git/t/t6010-merge-base.sh.

test_description='merge-base and parent list computation'

. ./test-lib.sh

M=1130000000
Z=+0000

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

doit () {
	OFFSET=$1 &&
	NAME=$2 &&
	shift 2 &&

	PARENTS= &&
	for P
	do
		PARENTS="${PARENTS}-p $P "
	done &&

	GIT_COMMITTER_DATE="$(($M + $OFFSET)) $Z" &&
	GIT_AUTHOR_DATE=$GIT_COMMITTER_DATE &&
	export GIT_COMMITTER_DATE GIT_AUTHOR_DATE &&

	commit=$(echo "$NAME" | git commit-tree "$(git write-tree)" $PARENTS) &&

	git update-ref "refs/tags/$NAME" "$commit" &&
	echo "$commit"
}

test_expect_success 'setup repository' '
	gust init repo &&
	cd repo
'

test_expect_success 'set up G and H graph' '
	cd repo &&
	# E---D---C---B---A
	# \"-_         \   \
	#  \  `---------G   \
	#   \                \
	#    F----------------H
	E=$(doit 5 E) &&
	D=$(doit 4 D $E) &&
	F=$(doit 6 F $E) &&
	C=$(doit 3 C $D) &&
	B=$(doit 2 B $C) &&
	A=$(doit 1 A $B) &&
	G=$(doit 7 G $B $E) &&
	H=$(doit 8 H $A $F)
'

test_expect_success 'default and --all merge-base G H' '
	cd repo &&
	git rev-parse B >expected &&
	MB=$(git merge-base G H) &&
	echo "$MB" >actual.single &&
	MB=$(git merge-base --all G H) &&
	echo "$MB" >actual.all &&
	test_cmp expected actual.single &&
	test_cmp expected actual.all
'

test_expect_success '--independent basic cases' '
	cd repo &&
	git rev-parse H >expected1 &&
	printf "%s\n%s\n" "$(git rev-parse H)" "$(git rev-parse G)" >expected2 &&
	parents=$(git merge-base --independent H) &&
	echo "$parents" >actual1 &&
	parents=$(git merge-base --independent A H G) &&
	printf "%s\n" $parents >actual2 &&
	test_cmp expected1 actual1 &&
	test_cmp expected2 actual2
'

test_expect_success '--is-ancestor returns status codes' '
	cd repo &&
	git merge-base --is-ancestor B H &&
	test_must_fail git merge-base --is-ancestor H B
'

test_expect_success '--is-ancestor handles equal commit' '
	cd repo &&
	git merge-base --is-ancestor G G
'

test_expect_success 'setup octopus/default difference graph' '
	cd repo &&
	# Build:
	# MMR - MM1 - MMA
	#   \    \
	#    \    MMB
	#     \
	#      MMC
	MMR=$(doit 20 MMR) &&
	MM1=$(doit 21 MM1 $MMR) &&
	MMA=$(doit 22 MMA $MM1) &&
	MMB=$(doit 23 MMB $MM1) &&
	MMC=$(doit 24 MMC $MMR)
'

test_expect_success 'merge-base A B C and --octopus differ' '
	cd repo &&
	git rev-parse MM1 >expected.default &&
	git rev-parse MMR >expected.octopus &&
	git merge-base --all MMA MMB MMC >actual.default &&
	git merge-base --all --octopus MMA MMB MMC >actual.octopus &&
	test_cmp expected.default actual.default &&
	test_cmp expected.octopus actual.octopus
'

test_expect_success 'disjoint histories report no merge base' '
	cd repo &&
	DIS1=$(doit 40 DIS1) &&
	DIS2=$(doit 41 DIS2) &&
	test_must_fail git merge-base "$DIS1" "$DIS2"
'

test_expect_success 'same commit repeated is its own merge base' '
	cd repo &&
	git rev-parse G >expected &&
	git merge-base G G >actual &&
	test_cmp expected actual
'

test_done
