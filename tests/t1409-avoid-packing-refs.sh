#!/bin/sh

test_description='avoid rewriting packed-refs unnecessarily'

. ./test-lib.sh

# Add an identifying mark to the packed-refs file header line.
mark_packed_refs () {
	sed -e "s/^\(#.*\)/\1 t1409 /" .git/packed-refs >.git/packed-refs.new &&
	mv .git/packed-refs.new .git/packed-refs
}

# Verify that the packed-refs file is still marked.
check_packed_refs_marked () {
	grep -q '^#.* t1409 ' .git/packed-refs
}

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "t@t" &&
	git commit --allow-empty -m "Commit A" &&
	git rev-parse HEAD >A_oid &&
	git commit --allow-empty -m "Commit B" &&
	git rev-parse HEAD >B_oid &&
	git commit --allow-empty -m "Commit C" &&
	git rev-parse HEAD >C_oid
'

test_expect_success 'do not create packed-refs file gratuitously' '
	A=$(cat A_oid) && B=$(cat B_oid) && C=$(cat C_oid) &&
	test_path_is_missing .git/packed-refs &&
	git update-ref refs/heads/foo $A &&
	test_path_is_missing .git/packed-refs &&
	git update-ref refs/heads/foo $B &&
	test_path_is_missing .git/packed-refs &&
	git update-ref refs/heads/foo $C $B &&
	test_path_is_missing .git/packed-refs &&
	git update-ref -d refs/heads/foo &&
	test_path_is_missing .git/packed-refs
'

test_expect_failure 'check that marking the packed-refs file works' '
	git for-each-ref >expected &&
	git pack-refs --all &&
	mark_packed_refs &&
	check_packed_refs_marked &&
	git for-each-ref >actual &&
	test_cmp expected actual &&
	git pack-refs --all &&
	! check_packed_refs_marked &&
	git for-each-ref >actual2 &&
	test_cmp expected actual2
'

test_expect_success 'leave packed-refs untouched on update of packed' '
	A=$(cat A_oid) && B=$(cat B_oid) &&
	git update-ref refs/heads/packed-update $A &&
	git pack-refs --all &&
	mark_packed_refs &&
	git update-ref refs/heads/packed-update $B &&
	check_packed_refs_marked
'

test_expect_success 'leave packed-refs untouched on checked update of packed' '
	A=$(cat A_oid) && B=$(cat B_oid) &&
	git update-ref refs/heads/packed-checked-update $A &&
	git pack-refs --all &&
	mark_packed_refs &&
	git update-ref refs/heads/packed-checked-update $B $A &&
	check_packed_refs_marked
'

test_expect_success 'leave packed-refs untouched on verify of packed' '
	A=$(cat A_oid) &&
	git update-ref refs/heads/packed-verify $A &&
	git pack-refs --all &&
	mark_packed_refs &&
	echo "verify refs/heads/packed-verify $A" | git update-ref --stdin &&
	check_packed_refs_marked
'

test_expect_failure 'touch packed-refs on delete of packed' '
	A=$(cat A_oid) &&
	git update-ref refs/heads/packed-delete $A &&
	git pack-refs --all &&
	mark_packed_refs &&
	git update-ref -d refs/heads/packed-delete &&
	! check_packed_refs_marked
'

test_expect_success 'leave packed-refs untouched on update of loose' '
	A=$(cat A_oid) && B=$(cat B_oid) &&
	git pack-refs --all &&
	git update-ref refs/heads/loose-update $A &&
	mark_packed_refs &&
	git update-ref refs/heads/loose-update $B &&
	check_packed_refs_marked
'

test_expect_success 'leave packed-refs untouched on checked update of loose' '
	A=$(cat A_oid) && B=$(cat B_oid) &&
	git pack-refs --all &&
	git update-ref refs/heads/loose-checked-update $A &&
	mark_packed_refs &&
	git update-ref refs/heads/loose-checked-update $B $A &&
	check_packed_refs_marked
'

test_expect_success 'leave packed-refs untouched on verify of loose' '
	A=$(cat A_oid) &&
	git pack-refs --all &&
	git update-ref refs/heads/loose-verify $A &&
	mark_packed_refs &&
	echo "verify refs/heads/loose-verify $A" | git update-ref --stdin &&
	check_packed_refs_marked
'

test_expect_success 'leave packed-refs untouched on delete of loose' '
	A=$(cat A_oid) &&
	git pack-refs --all &&
	git update-ref refs/heads/loose-delete $A &&
	mark_packed_refs &&
	git update-ref -d refs/heads/loose-delete &&
	check_packed_refs_marked
'

test_done
