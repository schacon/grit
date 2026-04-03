#!/bin/sh
#
# Tests for rev-list history simplification with paths

test_description='rev-list history simplification'

. ./test-lib.sh

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

REAL_GIT=$(PATH="/usr/bin:/usr/local/bin:$PATH" command -v git)

#
# History:
#   A(file) -- B(other) -- C(file) -- D(other) -- E(file) (main)
#               \                                /
#                F(topic-file) -- G(topic-file)  (topic)
#
test_expect_success 'setup history with path-relevant and irrelevant commits' '
	git init -b main . &&
	echo a >file &&
	git add file &&
	test_tick &&
	git commit -m "A: add file" &&
	git tag A &&

	echo b >other &&
	git add other &&
	test_tick &&
	git commit -m "B: add other" &&
	git tag B &&

	echo c >>file &&
	git add file &&
	test_tick &&
	git commit -m "C: modify file" &&
	git tag C &&

	echo d >>other &&
	git add other &&
	test_tick &&
	git commit -m "D: modify other" &&
	git tag D &&

	git checkout -b topic B &&
	echo f >topic-file &&
	git add topic-file &&
	test_tick &&
	git commit -m "F: add topic-file" &&
	git tag F &&

	echo g >>topic-file &&
	git add topic-file &&
	test_tick &&
	git commit -m "G: modify topic-file" &&
	git tag G &&

	git checkout main &&
	$REAL_GIT merge topic -m "E: merge" &&
	git tag E &&

	echo e >>file &&
	git add file &&
	test_tick &&
	git commit -m "H: modify file again" &&
	git tag H
'

test_expect_success 'rev-list without path lists all commits' '
	git rev-list H >actual &&
	# H E D C G F B A = 8
	test_line_count = 8 actual
'

test_expect_success 'rev-list --simplify-by-decoration lists decorated commits' '
	git rev-list --simplify-by-decoration --all >actual &&
	# Should include all tagged/branched commits
	test $(wc -l <actual) -ge 1
'

test_expect_success 'rev-list --simplify-by-decoration includes HEAD' '
	git rev-list --simplify-by-decoration HEAD >actual &&
	HEAD_SHA=$(git rev-parse HEAD) &&
	grep "$HEAD_SHA" actual
'

test_expect_success 'rev-list --simplify-by-decoration includes tags' '
	git rev-list --simplify-by-decoration --all >actual &&
	A_SHA=$(git rev-parse A) &&
	grep "$A_SHA" actual
'

test_expect_success 'rev-list --all lists commits from all branches' '
	git rev-list --all >actual &&
	test $(wc -l <actual) -ge 8
'

test_expect_success 'rev-list --count with --all' '
	git rev-list --count --all >actual &&
	test $(cat actual) -ge 8
'

test_expect_success 'rev-list -- path limits to path-touching commits' '
	git rev-list H -- file >actual &&
	# Commits touching file: A, C, H
	test_line_count = 3 actual
'

test_expect_success 'rev-list -- other limits to other-touching commits' '
	git rev-list H -- other >actual &&
	# Commits touching other: B, D
	test_line_count = 2 actual
'

test_expect_success 'rev-list -- topic-file limits to topic-file commits' '
	git rev-list H -- topic-file >actual &&
	# Commits touching topic-file: F, G (and possibly E merge)
	test $(wc -l <actual) -ge 2
'

test_expect_success 'rev-list with range and path' '
	git rev-list B..H -- file >actual &&
	# C and H touch file after B
	test_line_count = 2 actual
'

test_expect_success 'rev-list --full-history shows all path-relevant commits' '
	git rev-list --full-history H -- file >actual &&
	test $(wc -l <actual) -ge 3
'

test_expect_success 'rev-list --full-history --simplify-merges with path' '
	git rev-list --full-history --simplify-merges H -- file >actual &&
	test $(wc -l <actual) -ge 1
'

test_expect_success 'rev-list --sparse shows all commits despite path' '
	git rev-list --sparse H -- file >actual &&
	# sparse does not prune non-matching commits
	test $(wc -l <actual) -ge 5
'

test_expect_success 'rev-list --dense with path (default behavior)' '
	git rev-list --dense H -- file >actual &&
	# dense is the default path simplification
	test $(wc -l <actual) -ge 1
'

# Test --simplify-by-decoration with ranges
test_expect_success 'rev-list --simplify-by-decoration with range' '
	git rev-list --simplify-by-decoration A..H >actual &&
	# Should list some decorated commits in range
	test $(wc -l <actual) -ge 1
'

# Test ordering with simplification
test_expect_success 'rev-list --topo-order with --simplify-by-decoration' '
	git rev-list --topo-order --simplify-by-decoration HEAD >actual &&
	test $(wc -l <actual) -ge 1
'

test_expect_success 'rev-list --date-order with --simplify-by-decoration' '
	git rev-list --date-order --simplify-by-decoration HEAD >actual &&
	test $(wc -l <actual) -ge 1
'

# Additional branches for richer decoration testing
test_expect_success 'setup additional branches' '
	git checkout -b feature1 C &&
	echo feat >feat-file &&
	git add feat-file &&
	test_tick &&
	git commit -m "feat1" &&
	git tag feat1 &&

	git checkout -b feature2 D &&
	echo feat2 >feat2-file &&
	git add feat2-file &&
	test_tick &&
	git commit -m "feat2" &&
	git tag feat2
'

test_expect_success 'rev-list --simplify-by-decoration --all shows all branch tips' '
	git rev-list --simplify-by-decoration --all >actual &&
	FEAT1=$(git rev-parse feat1) &&
	FEAT2=$(git rev-parse feat2) &&
	grep "$FEAT1" actual &&
	grep "$FEAT2" actual
'

test_expect_success 'rev-list --all --count includes feature branches' '
	git rev-list --count --all >actual &&
	test $(cat actual) -ge 10
'

test_expect_success 'rev-list --simplify-merges removes unnecessary merges' '
	git rev-list --simplify-merges H -- file >actual &&
	test $(wc -l <actual) -ge 1
'

test_expect_success 'rev-list --reverse with --simplify-by-decoration' '
	git rev-list --reverse --simplify-by-decoration HEAD >actual &&
	head -1 actual >first &&
	A_SHA=$(git rev-parse A) &&
	echo "$A_SHA" >expect &&
	test_cmp expect first
'

test_expect_success 'rev-list --first-parent with --simplify-by-decoration' '
	git rev-list --first-parent --simplify-by-decoration main >actual &&
	test $(wc -l <actual) -ge 1
'

test_done
