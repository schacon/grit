#!/bin/sh
# Tests for 'grit merge-file' — three-way file merge.
# (merge-tree is not implemented; merge-file provides three-way file merging.)

test_description='grit merge-file'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Clean merge (no conflicts) ───────────────────────────────────────────────

test_expect_success 'setup clean merge files' '
	cat >base.txt <<-\EOF &&
	line 1
	line 2
	line 3
	line 4
	line 5
	EOF
	cp base.txt ours.txt &&
	cp base.txt theirs.txt &&
	# ours changes line 1
	sed -i "s/line 1/line 1 modified by ours/" ours.txt &&
	# theirs changes line 5
	sed -i "s/line 5/line 5 modified by theirs/" theirs.txt
'

test_expect_success 'merge-file with no conflicts succeeds (exit 0)' '
	cp ours.txt current.txt &&
	git merge-file current.txt base.txt theirs.txt &&
	grep "line 1 modified by ours" current.txt &&
	grep "line 5 modified by theirs" current.txt
'

test_expect_success 'merge-file -p sends result to stdout' '
	cp ours.txt current2.txt &&
	git merge-file -p current2.txt base.txt theirs.txt >merged &&
	grep "line 1 modified by ours" merged &&
	grep "line 5 modified by theirs" merged
'

test_expect_success 'merge-file -p does not modify input file' '
	cp ours.txt current3.txt &&
	git merge-file -p current3.txt base.txt theirs.txt >merged &&
	test_cmp ours.txt current3.txt
'

test_expect_success 'clean merge preserves unchanged lines' '
	cp ours.txt current4.txt &&
	git merge-file current4.txt base.txt theirs.txt &&
	grep "line 2" current4.txt &&
	grep "line 3" current4.txt &&
	grep "line 4" current4.txt
'

# ── Conflicting merge ───────────────────────────────────────────────────────

test_expect_success 'setup conflicting files' '
	printf "line 1\nline 2\nline 3\n" >base_c.txt &&
	printf "line 1\nline 2 ours\nline 3\n" >ours_c.txt &&
	printf "line 1\nline 2 theirs\nline 3\n" >theirs_c.txt
'

test_expect_success 'merge-file with conflicts returns nonzero' '
	cp ours_c.txt conflict.txt &&
	test_must_fail git merge-file conflict.txt base_c.txt theirs_c.txt
'

test_expect_success 'conflicted output contains conflict markers' '
	cp ours_c.txt conflict2.txt &&
	test_must_fail git merge-file conflict2.txt base_c.txt theirs_c.txt &&
	grep "<<<<<<" conflict2.txt &&
	grep ">>>>>>" conflict2.txt
'

test_expect_success 'conflicted output has both versions' '
	cp ours_c.txt conflict3.txt &&
	test_must_fail git merge-file conflict3.txt base_c.txt theirs_c.txt &&
	grep "line 2 ours" conflict3.txt &&
	grep "line 2 theirs" conflict3.txt
'

test_expect_success 'merge-file -p with conflict sends to stdout' '
	cp ours_c.txt conflict4.txt &&
	test_must_fail git merge-file -p conflict4.txt base_c.txt theirs_c.txt >merged_c &&
	grep "<<<<<<" merged_c &&
	grep ">>>>>>" merged_c
'

# ── --ours / --theirs / --union ──────────────────────────────────────────────

test_expect_success 'merge-file --ours resolves with our version' '
	cp ours_c.txt resolve_ours.txt &&
	git merge-file --ours resolve_ours.txt base_c.txt theirs_c.txt &&
	grep "line 2 ours" resolve_ours.txt &&
	! grep "line 2 theirs" resolve_ours.txt
'

test_expect_success 'merge-file --theirs resolves with their version' '
	cp ours_c.txt resolve_theirs.txt &&
	git merge-file --theirs resolve_theirs.txt base_c.txt theirs_c.txt &&
	grep "line 2 theirs" resolve_theirs.txt &&
	! grep "line 2 ours" resolve_theirs.txt
'

test_expect_success 'merge-file --union includes both without markers' '
	cp ours_c.txt resolve_union.txt &&
	git merge-file --union resolve_union.txt base_c.txt theirs_c.txt &&
	! grep "<<<<<<" resolve_union.txt &&
	grep "line 2 ours" resolve_union.txt &&
	grep "line 2 theirs" resolve_union.txt
'

# ── --diff3 ──────────────────────────────────────────────────────────────────

test_expect_success 'merge-file --diff3 shows base version in conflicts' '
	cp ours_c.txt diff3.txt &&
	test_must_fail git merge-file --diff3 diff3.txt base_c.txt theirs_c.txt &&
	grep "|||||||" diff3.txt &&
	grep "line 2$" diff3.txt
'

# ── --zdiff3 ─────────────────────────────────────────────────────────────────

test_expect_success 'merge-file --zdiff3 produces valid output' '
	cp ours_c.txt zdiff3.txt &&
	test_must_fail git merge-file --zdiff3 zdiff3.txt base_c.txt theirs_c.txt &&
	grep "<<<<<<" zdiff3.txt &&
	grep ">>>>>>" zdiff3.txt
'

# ── Labels (-L) ──────────────────────────────────────────────────────────────

test_expect_success 'merge-file -L sets custom conflict labels' '
	cp ours_c.txt labeled.txt &&
	test_must_fail git merge-file -L "OURS" -L "BASE" -L "THEIRS" labeled.txt base_c.txt theirs_c.txt &&
	grep "<<<<<<< OURS" labeled.txt &&
	grep ">>>>>>> THEIRS" labeled.txt
'

test_expect_success 'merge-file --diff3 -L shows base label' '
	cp ours_c.txt labeled_diff3.txt &&
	test_must_fail git merge-file --diff3 -L "OURS" -L "BASE" -L "THEIRS" labeled_diff3.txt base_c.txt theirs_c.txt &&
	grep "||||||| BASE" labeled_diff3.txt
'

# ── --marker-size ────────────────────────────────────────────────────────────

test_expect_success 'merge-file --marker-size changes marker length' '
	cp ours_c.txt marker.txt &&
	test_must_fail git merge-file --marker-size 10 marker.txt base_c.txt theirs_c.txt &&
	grep "<<<<<<<<<< " marker.txt &&
	grep ">>>>>>>>>> " marker.txt
'

# ── --quiet ──────────────────────────────────────────────────────────────────

test_expect_success 'merge-file -q suppresses warnings on conflict' '
	cp ours_c.txt quiet.txt &&
	test_must_fail git merge-file -q quiet.txt base_c.txt theirs_c.txt 2>stderr &&
	test_must_be_empty stderr
'

# ── Identical files (trivial merge) ─────────────────────────────────────────

test_expect_success 'merge-file with identical files succeeds' '
	echo "same content" >same1.txt &&
	echo "same content" >same2.txt &&
	echo "same content" >same3.txt &&
	git merge-file same1.txt same2.txt same3.txt &&
	echo "same content" >expect &&
	test_cmp expect same1.txt
'

test_expect_success 'merge-file with all-empty files succeeds' '
	>empty1.txt &&
	>empty2.txt &&
	>empty3.txt &&
	git merge-file empty1.txt empty2.txt empty3.txt &&
	test_must_be_empty empty1.txt
'

# ── Multi-line complex merge ─────────────────────────────────────────────────

test_expect_success 'setup complex merge files' '
	printf "alpha\nbravo\ncharlie\ndelta\necho\nfoxtrot\ngolf\nhotel\n" >base_complex.txt &&
	printf "alpha\nbravo MODIFIED\ncharlie\ndelta\necho\nfoxtrot\ngolf\nhotel\n" >ours_complex.txt &&
	printf "alpha\nbravo\ncharlie\ndelta\necho\nfoxtrot MODIFIED\ngolf\nhotel\n" >theirs_complex.txt
'

test_expect_success 'complex merge with no conflicts combines changes' '
	cp ours_complex.txt result.txt &&
	git merge-file result.txt base_complex.txt theirs_complex.txt &&
	grep "bravo MODIFIED" result.txt &&
	grep "foxtrot MODIFIED" result.txt
'

test_expect_success 'complex merge preserves unchanged lines' '
	cp ours_complex.txt result2.txt &&
	git merge-file result2.txt base_complex.txt theirs_complex.txt &&
	grep "alpha" result2.txt &&
	grep "charlie" result2.txt &&
	grep "delta" result2.txt &&
	grep "echo" result2.txt &&
	grep "golf" result2.txt &&
	grep "hotel" result2.txt
'

test_expect_success 'merge-file with addition on one side' '
	cat >base_add.txt <<-\EOF &&
	line A
	line B
	EOF
	cat >ours_add.txt <<-\EOF &&
	line A
	line B
	line C added by ours
	EOF
	cp base_add.txt theirs_add.txt &&
	cp ours_add.txt result_add.txt &&
	git merge-file result_add.txt base_add.txt theirs_add.txt &&
	grep "line C added by ours" result_add.txt
'

test_expect_success 'merge-file with deletion on one side' '
	cat >base_del.txt <<-\EOF &&
	line X
	line Y
	line Z
	EOF
	cat >ours_del.txt <<-\EOF &&
	line X
	line Z
	EOF
	cp base_del.txt theirs_del.txt &&
	cp ours_del.txt result_del.txt &&
	git merge-file result_del.txt base_del.txt theirs_del.txt &&
	! grep "line Y" result_del.txt &&
	grep "line X" result_del.txt &&
	grep "line Z" result_del.txt
'

# ── Missing files ────────────────────────────────────────────────────────────

test_expect_success 'merge-file with nonexistent file fails' '
	echo "content" >exists.txt &&
	test_must_fail git merge-file exists.txt nonexistent.txt exists.txt 2>err &&
	test -s err
'

test_done
