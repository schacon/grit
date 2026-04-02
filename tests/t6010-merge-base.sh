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
	grit init repo &&
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

test_expect_success 'unsynchronized clocks' '
	cd repo &&
	S=$(doit 0 S) &&
	C0=$(doit -3 C0 $S) &&
	C1=$(doit -2 C1 $C0) &&
	C2=$(doit -1 C2 $C1) &&
	L0=$(doit 1 L0 $S) &&
	L1=$(doit 2 L1 $L0) &&
	L2=$(doit 3 L2 $L1) &&
	R0=$(doit 1 R0 $S) &&
	R1=$(doit 2 R1 $R0) &&
	R2=$(doit 3 R2 $R1) &&
	PL=$(doit 4 PL $L2 $C2) &&
	PR=$(doit 4 PR $C2 $R2) &&
	git rev-parse C2 >expected &&
	MB=$(git merge-base PL PR) &&
	echo "$MB" >actual.single &&
	MB=$(git merge-base --all PL PR) &&
	echo "$MB" >actual.all &&
	test_cmp expected actual.single &&
	test_cmp expected actual.all
'

test_expect_success '--independent with unsynchronized clocks' '
	cd repo &&
	IB=$(doit 0 IB) &&
	I1=$(doit -10 I1 $IB) &&
	I2=$(doit -9 I2 $I1) &&
	I3=$(doit -8 I3 $I2) &&
	I4=$(doit -7 I4 $I3) &&
	I5=$(doit -6 I5 $I4) &&
	I6=$(doit -5 I6 $I5) &&
	I7=$(doit -4 I7 $I6) &&
	I8=$(doit -3 I8 $I7) &&
	IH=$(doit -2 IH $I8) &&
	echo "$IH" >expected &&
	git merge-base --independent IB IH >actual &&
	test_cmp expected actual
'

test_expect_success 'linear chain merge-base' '
	cd repo &&
	LA=$(doit 50 LA) &&
	LB=$(doit 51 LB $LA) &&
	LC=$(doit 52 LC $LB) &&
	git rev-parse LA >expected &&
	git merge-base LA LC >actual &&
	test_cmp expected actual
'

test_expect_success 'diamond merge-base' '
	cd repo &&
	D1=$(doit 60 D1) &&
	D2=$(doit 61 D2 $D1) &&
	D3=$(doit 62 D3 $D1) &&
	git rev-parse D1 >expected &&
	git merge-base D2 D3 >actual &&
	test_cmp expected actual
'

test_expect_success 'multiple merge bases with --all' '
	cd repo &&
	M1=$(doit 70 M1) &&
	M2=$(doit 71 M2 $M1) &&
	M3=$(doit 72 M3 $M1) &&
	M4=$(doit 73 M4 $M2 $M3) &&
	M5=$(doit 74 M5 $M2 $M3) &&
	git merge-base --all M4 M5 >actual &&
	sort actual >actual.sorted &&
	printf "%s\n%s\n" "$M2" "$M3" | sort >expected.sorted &&
	test_cmp expected.sorted actual.sorted
'

test_expect_success '--is-ancestor with linear chain' '
	cd repo &&
	git merge-base --is-ancestor LA LC &&
	test_must_fail git merge-base --is-ancestor LC LA
'

test_expect_success '--is-ancestor with disjoint histories' '
	cd repo &&
	test_must_fail git merge-base --is-ancestor DIS1 DIS2 &&
	test_must_fail git merge-base --is-ancestor DIS2 DIS1
'

test_expect_success '--independent filters ancestors from set' '
	cd repo &&
	git merge-base --independent E D F >actual &&
	sort actual >actual.sorted &&
	printf "%s\n%s\n" "$(git rev-parse D)" "$(git rev-parse F)" | sort >expected.sorted &&
	test_cmp expected.sorted actual.sorted
'

test_expect_success 'merge-base of direct parent and child' '
	cd repo &&
	git rev-parse E >expected &&
	git merge-base D E >actual &&
	test_cmp expected actual
'

# --- Additional merge-base tests ---

test_expect_success '--octopus with two commits same as default' '
	cd repo &&
	git merge-base G H >expected &&
	git merge-base --octopus G H >actual &&
	test_cmp expected actual
'

test_expect_success '--octopus with three commits' '
	cd repo &&
	git merge-base --octopus G H A >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success '--octopus on linear chain' '
	cd repo &&
	git rev-parse LA >expected &&
	git merge-base --octopus LA LB LC >actual &&
	test_cmp expected actual
'

test_expect_success '--is-ancestor with three generations' '
	cd repo &&
	git merge-base --is-ancestor E C &&
	git merge-base --is-ancestor E B &&
	git merge-base --is-ancestor E A
'

test_expect_success '--is-ancestor fails for unrelated commits' '
	cd repo &&
	test_must_fail git merge-base --is-ancestor DIS1 A
'

test_expect_success '--is-ancestor with merge commit lineage' '
	cd repo &&
	git merge-base --is-ancestor F H &&
	git merge-base --is-ancestor E H
'

test_expect_success 'merge-base with 3 commits takes pairwise best' '
	cd repo &&
	git merge-base A G H >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success 'merge-base with identical refs' '
	cd repo &&
	git rev-parse A >expected &&
	git merge-base A A >actual &&
	test_cmp expected actual
'

test_expect_success '--independent with single ref returns it' '
	cd repo &&
	git rev-parse H >expected &&
	git merge-base --independent H >actual &&
	test_cmp expected actual
'

test_expect_success '--independent removes ancestors from list' '
	cd repo &&
	git merge-base --independent A B C >actual &&
	# A is tip, B and C are ancestors of A
	git rev-parse A >expected &&
	test_cmp expected actual
'

test_expect_success '--independent with all independent refs keeps all' '
	cd repo &&
	git merge-base --independent DIS1 DIS2 >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = 2
'

test_expect_success 'merge-base symmetric: order does not matter' '
	cd repo &&
	git merge-base G H >gh &&
	git merge-base H G >hg &&
	test_cmp gh hg
'

test_expect_success 'merge-base of root commit with descendant is root' '
	cd repo &&
	git rev-parse E >expected &&
	git merge-base E A >actual &&
	test_cmp expected actual
'

test_expect_success '--is-ancestor reflexive: commit is own ancestor' '
	cd repo &&
	git merge-base --is-ancestor A A
'

test_expect_success 'setup criss-cross merge' '
	cd repo &&
	CC1=$(doit 80 CC1) &&
	CC2=$(doit 81 CC2 $CC1) &&
	CC3=$(doit 82 CC3 $CC1) &&
	CC4=$(doit 83 CC4 $CC2 $CC3) &&
	CC5=$(doit 84 CC5 $CC3 $CC2) &&
	CC6=$(doit 85 CC6 $CC4) &&
	CC7=$(doit 86 CC7 $CC5)
'

test_expect_success 'criss-cross merge has multiple merge bases' '
	cd repo &&
	git merge-base --all CC6 CC7 >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" -ge 2
'

test_expect_success '--octopus with criss-cross descendants' '
	cd repo &&
	git merge-base --octopus CC6 CC7 >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success '--is-ancestor through criss-cross' '
	cd repo &&
	git merge-base --is-ancestor CC1 CC6 &&
	git merge-base --is-ancestor CC1 CC7
'

test_expect_success 'merge-base with tag names' '
	cd repo &&
	git merge-base E D >actual &&
	git rev-parse E >expected &&
	test_cmp expected actual
'

test_expect_success '--independent preserves only tips in long chain' '
	cd repo &&
	git merge-base --independent LA LB LC >actual &&
	git rev-parse LC >expected &&
	test_cmp expected actual
'

test_expect_success 'merge-base diamond with --all finds single base' '
	cd repo &&
	git merge-base --all D2 D3 >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = 1 &&
	git rev-parse D1 >expected &&
	test_cmp expected actual
'

test_done
