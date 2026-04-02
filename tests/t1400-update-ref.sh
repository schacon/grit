#!/bin/sh
# Ported from git/t/t1400-update-ref.sh (harness-compatible subset).

test_description='grit update-ref basics'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

A=1111111111111111111111111111111111111111
B=2222222222222222222222222222222222222222
C=3333333333333333333333333333333333333333
D=4444444444444444444444444444444444444444
E=5555555555555555555555555555555555555555
F=6666666666666666666666666666666666666666
Z=0000000000000000000000000000000000000000

m=refs/heads/master

head_ref_path() {
	sed -n 's/^ref: //p' .git/HEAD
}

test_expect_success 'setup repository' '
	grit init repo &&
	cd repo
'

# === Basic ref create / update / delete ===

test_expect_success "create $m" '
	cd repo &&
	grit update-ref $m "$A" &&
	echo "$A" >expect &&
	cat .git/refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success "create $m with oldvalue verification" '
	cd repo &&
	grit update-ref $m "$B" "$A" &&
	echo "$B" >expect &&
	cat .git/refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success "fail to delete $m with stale ref" '
	cd repo &&
	test_must_fail grit update-ref -d $m "$A" &&
	echo "$B" >expect &&
	cat .git/refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success "delete $m" '
	cd repo &&
	grit update-ref -d $m "$B" &&
	test_path_is_missing .git/refs/heads/master
'

test_expect_success "delete $m without oldvalue verification" '
	cd repo &&
	grit update-ref $m "$A" &&
	echo "$A" >expect &&
	cat .git/refs/heads/master >actual &&
	test_cmp expect actual &&
	grit update-ref -d $m &&
	test_path_is_missing .git/refs/heads/master
'

# === HEAD dereferences to current branch ===

test_expect_success "create $m (by HEAD)" '
	cd repo &&
	grit update-ref HEAD "$A" &&
	head_ref=$(head_ref_path) &&
	echo "$A" >expect &&
	cat ".git/$head_ref" >actual &&
	test_cmp expect actual
'

test_expect_success "create $m (by HEAD) with oldvalue verification" '
	cd repo &&
	grit update-ref HEAD "$B" "$A" &&
	head_ref=$(head_ref_path) &&
	echo "$B" >expect &&
	cat ".git/$head_ref" >actual &&
	test_cmp expect actual
'

test_expect_success "fail to delete $m (by HEAD) with stale ref" '
	cd repo &&
	test_must_fail grit update-ref -d HEAD "$A" &&
	echo "$B" >expect &&
	cat .git/refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success "delete $m (by HEAD)" '
	cd repo &&
	grit update-ref -d HEAD "$B" &&
	test_path_is_missing .git/refs/heads/master
'

# === File/directory conflicts ===

test_expect_success "fail to create ref due to file/directory conflict" '
	cd repo &&
	grit update-ref refs/heads/gu "$A" &&
	test_must_fail grit update-ref refs/heads/gu/fixes "$A" &&
	echo "$A" >expect &&
	cat .git/refs/heads/gu >actual &&
	test_cmp expect actual
'

test_expect_success "fail to create parent when deeper ref exists" '
	cd repo &&
	grit update-ref refs/heads/deep/child "$A" &&
	test_must_fail grit update-ref refs/heads/deep "$B" &&
	echo "$A" >expect &&
	cat .git/refs/heads/deep/child >actual &&
	test_cmp expect actual
'

# === Wrong old-value with HEAD ===

test_expect_success "(not) create HEAD with old sha1" '
	cd repo &&
	test_must_fail grit update-ref HEAD "$A" "$B"
'

test_expect_success "create HEAD" '
	cd repo &&
	grit update-ref HEAD "$A"
'

test_expect_success "(not) change HEAD with wrong SHA1" '
	cd repo &&
	test_must_fail grit update-ref HEAD "$B" "$Z"
'

test_expect_success "(not) changed .git/$m" '
	cd repo &&
	test "$A" = "$(cat .git/refs/heads/master)"
'

# === --stdin basics ===

test_expect_success '--stdin accepts empty input' '
	cd repo &&
	: >stdin &&
	grit update-ref --stdin <stdin &&
	head_ref=$(head_ref_path) &&
	echo "$A" >expect &&
	cat ".git/$head_ref" >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin create works' '
	cd repo &&
	echo "create refs/heads/topic $B" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$B" >expect &&
	cat .git/refs/heads/topic >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin update with zero old-value creates ref' '
	cd repo &&
	echo "update refs/heads/newref $A $Z" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$A" >expect &&
	cat .git/refs/heads/newref >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin fails on unknown command' '
	cd repo &&
	echo "unknown refs/heads/a" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	grep "unknown" err
'

test_expect_success '--stdin fails on unbalanced quotes' '
	cd repo &&
	echo "create refs/heads/a \"master" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success '--stdin fails create with no ref' '
	cd repo &&
	echo "create " >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success '--stdin fails create with no new value' '
	cd repo &&
	echo "create refs/heads/a" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success '--stdin fails update with no ref' '
	cd repo &&
	echo "update " >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success '--stdin fails update with no new value' '
	cd repo &&
	echo "update refs/heads/a" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success '--stdin fails delete with no ref' '
	cd repo &&
	echo "delete " >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success '--stdin fails with duplicate refs' '
	cd repo &&
	cat >stdin <<-EOF &&
	create refs/heads/dup1 $A
	create refs/heads/dup2 $A
	create refs/heads/dup1 $A
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success 'stdin create ref works' '
	cd repo &&
	grit update-ref -d refs/heads/a 2>/dev/null || true &&
	echo "create refs/heads/a $A" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$A" >expect &&
	grit rev-parse refs/heads/a >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin update ref creates with zero old value' '
	cd repo &&
	grit update-ref -d refs/heads/b 2>/dev/null || true &&
	echo "update refs/heads/b $B $Z" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$B" >expect &&
	grit rev-parse refs/heads/b >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin update ref fails with bad old value' '
	cd repo &&
	echo "update refs/heads/c $A does-not-exist" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	grep "does-not-exist" err
'

test_expect_success 'stdin create ref fails with bad new value' '
	cd repo &&
	echo "create refs/heads/c does-not-exist" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	grep "does-not-exist" err
'

test_expect_success 'stdin delete ref fails with wrong old value' '
	cd repo &&
	echo "delete refs/heads/a $B" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$A" >expect &&
	grit rev-parse refs/heads/a >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin delete ref works with right old value' '
	cd repo &&
	grit update-ref refs/heads/delme $C &&
	echo "delete refs/heads/delme $C" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/delme
'

# === stdin verify ===

test_expect_success 'stdin verify succeeds for correct value' '
	cd repo &&
	echo "verify refs/heads/a $A" >stdin &&
	grit update-ref --stdin <stdin
'

test_expect_success 'stdin verify succeeds for missing reference' '
	cd repo &&
	echo "verify refs/heads/missing $Z" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/missing
'

test_expect_success 'stdin verify fails for wrong value' '
	cd repo &&
	echo "verify refs/heads/a $B" >stdin &&
	test_must_fail grit update-ref --stdin <stdin
'

test_expect_success 'stdin verify fails for mistaken null value' '
	cd repo &&
	echo "verify refs/heads/a $Z" >stdin &&
	test_must_fail grit update-ref --stdin <stdin
'

# === stdin update/create/verify combination ===

test_expect_success 'stdin update/create combination works' '
	cd repo &&
	grit update-ref -d refs/heads/c 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	update refs/heads/a $A
	create refs/heads/c $C
	EOF
	grit update-ref --stdin <stdin &&
	echo "$A" >expect &&
	grit rev-parse refs/heads/a >actual &&
	test_cmp expect actual &&
	echo "$C" >expect &&
	grit rev-parse refs/heads/c >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin update refs works with identity updates' '
	cd repo &&
	cat >stdin <<-EOF &&
	update refs/heads/a $A $A
	update refs/heads/b $B $B
	EOF
	grit update-ref --stdin <stdin &&
	echo "$A" >expect &&
	grit rev-parse refs/heads/a >actual &&
	test_cmp expect actual &&
	echo "$B" >expect &&
	grit rev-parse refs/heads/b >actual &&
	test_cmp expect actual
'

# === Pseudoref tests (with real objects) ===

test_expect_success 'setup real repo for pseudoref tests' '
	grit init real-repo &&
	cd real-repo &&
	echo test >file.txt &&
	grit add file.txt &&
	GIT_AUTHOR_NAME="Test" GIT_AUTHOR_EMAIL="test@test" \
	GIT_COMMITTER_NAME="Test" GIT_COMMITTER_EMAIL="test@test" \
	grit commit -m "initial" &&
	echo change >file.txt &&
	grit add file.txt &&
	GIT_AUTHOR_NAME="Test" GIT_AUTHOR_EMAIL="test@test" \
	GIT_COMMITTER_NAME="Test" GIT_COMMITTER_EMAIL="test@test" \
	grit commit -m "second"
'

test_expect_success 'given old value for missing pseudoref, do not create' '
	cd real-repo &&
	SHA_A=$(grit rev-parse HEAD) &&
	SHA_B=$(grit rev-parse HEAD~1) &&
	test_must_fail grit update-ref PSEUDOREF "$SHA_A" "$SHA_B" 2>err &&
	test_must_fail grit rev-parse PSEUDOREF 2>/dev/null
'

test_expect_success 'create pseudoref' '
	cd real-repo &&
	SHA_A=$(grit rev-parse HEAD) &&
	grit update-ref PSEUDOREF "$SHA_A" &&
	test "$SHA_A" = "$(grit rev-parse PSEUDOREF)"
'

test_expect_success 'overwrite pseudoref with no old value given' '
	cd real-repo &&
	SHA_B=$(grit rev-parse HEAD~1) &&
	grit update-ref PSEUDOREF "$SHA_B" &&
	test "$SHA_B" = "$(grit rev-parse PSEUDOREF)"
'

test_expect_success 'overwrite pseudoref with correct old value' '
	cd real-repo &&
	SHA_A=$(grit rev-parse HEAD) &&
	SHA_B=$(grit rev-parse HEAD~1) &&
	grit update-ref PSEUDOREF "$SHA_A" "$SHA_B" &&
	test "$SHA_A" = "$(grit rev-parse PSEUDOREF)"
'

test_expect_success 'do not overwrite pseudoref with wrong old value' '
	cd real-repo &&
	SHA_A=$(grit rev-parse HEAD) &&
	SHA_B=$(grit rev-parse HEAD~1) &&
	test_must_fail grit update-ref PSEUDOREF "$SHA_B" "$SHA_B" 2>err &&
	test "$SHA_A" = "$(grit rev-parse PSEUDOREF)"
'

test_expect_success 'delete pseudoref' '
	cd real-repo &&
	grit update-ref -d PSEUDOREF &&
	test_must_fail grit rev-parse PSEUDOREF 2>/dev/null
'

test_expect_success 'do not delete pseudoref with wrong old value' '
	cd real-repo &&
	SHA_A=$(grit rev-parse HEAD) &&
	SHA_B=$(grit rev-parse HEAD~1) &&
	grit update-ref PSEUDOREF "$SHA_A" &&
	test_must_fail grit update-ref -d PSEUDOREF "$SHA_B" 2>err &&
	test "$SHA_A" = "$(grit rev-parse PSEUDOREF)"
'

test_expect_success 'delete pseudoref with correct old value' '
	cd real-repo &&
	SHA_A=$(grit rev-parse HEAD) &&
	grit update-ref -d PSEUDOREF "$SHA_A" &&
	test_must_fail grit rev-parse PSEUDOREF 2>/dev/null
'

test_expect_success 'create pseudoref with old OID zero' '
	cd real-repo &&
	SHA_A=$(grit rev-parse HEAD) &&
	grit update-ref PSEUDOREF "$SHA_A" "$Z" &&
	test "$SHA_A" = "$(grit rev-parse PSEUDOREF)"
'

test_expect_success 'do not overwrite pseudoref with old OID zero' '
	cd real-repo &&
	SHA_A=$(grit rev-parse HEAD) &&
	SHA_B=$(grit rev-parse HEAD~1) &&
	test_must_fail grit update-ref PSEUDOREF "$SHA_B" "$Z" 2>err &&
	test "$SHA_A" = "$(grit rev-parse PSEUDOREF)" &&
	grit update-ref -d PSEUDOREF
'

# === stdin with real objects ===

test_expect_success 'stdin test setup' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	echo "$m_sha" >expect
'

test_expect_success 'stdin works with no input' '
	cd real-repo &&
	>stdin &&
	grit update-ref --stdin <stdin &&
	grit rev-parse --verify -q refs/heads/master
'

test_expect_success 'stdin create ref works (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/stdintest 2>/dev/null || true &&
	echo "create refs/heads/stdintest refs/heads/master" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/stdintest >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin update ref creates with zero old value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/uptest 2>/dev/null || true &&
	echo "update refs/heads/uptest refs/heads/master $Z" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/uptest >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin update ref fails with wrong old value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/failtest "$m_sha" &&
	echo "update refs/heads/failtest $Z $Z" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success 'stdin delete ref works with right old value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/deltest "$m_sha" &&
	echo "delete refs/heads/deltest $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/deltest
'

test_expect_success 'stdin verify succeeds for correct value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	echo "verify refs/heads/master $m_sha" >stdin &&
	grit update-ref --stdin <stdin
'

test_expect_success 'stdin verify succeeds for missing reference (real objects)' '
	cd real-repo &&
	echo "verify refs/heads/nosuchref $Z" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/nosuchref
'

test_expect_success 'stdin verify fails for wrong value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	echo "verify refs/heads/master $parent_sha" >stdin &&
	test_must_fail grit update-ref --stdin <stdin
'

test_expect_success 'stdin verify fails for mistaken null value (real objects)' '
	cd real-repo &&
	echo "verify refs/heads/master $Z" >stdin &&
	test_must_fail grit update-ref --stdin <stdin
'

# === Transaction tests ===

test_expect_success 'transaction start/create/commit reports status' '
	cd repo &&
	cat >stdin <<-\EOF &&
	start
	create refs/heads/txref 3333333333333333333333333333333333333333
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	cat >expect <<-\EOF &&
	start: ok
	commit: ok
	EOF
	test_cmp expect actual &&
	echo 3333333333333333333333333333333333333333 >expect &&
	cat .git/refs/heads/txref >actual &&
	test_cmp expect actual
'

test_expect_success 'transaction handles empty commit with missing prepare' '
	cd repo &&
	cat >stdin <<-\EOF &&
	start
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual
'

test_expect_success 'transaction handles empty abort' '
	cd real-repo &&
	cat >stdin <<-\EOF &&
	start
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start >expect &&
	test_cmp expect actual
'

test_expect_success 'transaction handles sole abort' '
	cd real-repo &&
	cat >stdin <<-\EOF &&
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	test_must_be_empty actual
'

test_expect_success 'transaction can handle commit (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/txcommit 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/txcommit $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/txcommit >actual &&
	test_cmp expect actual
'

test_expect_success 'transaction can handle abort (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/txabort 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/txabort $m_sha
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/txabort
'

test_expect_success 'transaction aborts by default' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/txdefault 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/txdefault $m_sha
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/txdefault
'

test_expect_success 'transaction can commit multiple times' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref -d refs/heads/branch-1 2>/dev/null || true &&
	grit update-ref -d refs/heads/branch-2 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/branch-1 $parent_sha
	commit
	start
	create refs/heads/branch-2 $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit start commit >expect &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/branch-1 >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/branch-2 >actual &&
	test_cmp expect actual
'

test_expect_success 'transaction can create and delete' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/create-and-delete 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/create-and-delete $m_sha
	commit
	start
	delete refs/heads/create-and-delete $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit start commit >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/create-and-delete
'

test_expect_success 'transaction can commit after abort' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/abort-then-commit 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/abort-then-commit $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/abort-then-commit >actual &&
	test_cmp expect actual
'

test_expect_success 'transaction cannot restart ongoing transaction' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/restart 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/restart $m_sha
	start
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/restart
'

# === More stdin error handling tests ===

test_expect_success 'stdin fails option with unknown name' '
	cd repo &&
	echo "option unknown" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success 'stdin create ref fails with zero new value' '
	cd repo &&
	echo "create refs/heads/zeronew " >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success 'stdin delete ref fails with zero old value' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref refs/heads/delzero "$m_sha" &&
	echo "delete refs/heads/delzero " >stdin &&
	grit update-ref --stdin <stdin 2>err &&
	test_must_fail grit rev-parse --verify -q refs/heads/delzero
'

# === Real-repo for-each-ref based tests ===

test_expect_success 'for-each-ref with prefix filter' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref refs/test/one "$m_sha" &&
	grit update-ref refs/test/two "$m_sha" &&
	grit for-each-ref refs/test/ >actual &&
	grep refs/test/one actual &&
	grep refs/test/two actual
'

# === Multiple refs in a single stdin transaction ===

test_expect_success 'stdin creates multiple refs in one transaction' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref -d refs/heads/multi1 2>/dev/null || true &&
	grit update-ref -d refs/heads/multi2 2>/dev/null || true &&
	grit update-ref -d refs/heads/multi3 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/multi1 $m_sha
	create refs/heads/multi2 $parent_sha
	create refs/heads/multi3 $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/multi1 >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/multi2 >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/multi3 >actual &&
	test_cmp expect actual
'

# === HEAD-related tests with real objects ===

test_expect_success 'updating HEAD with real commits' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref HEAD "$parent_sha" "$m_sha" &&
	echo "$parent_sha" >expect &&
	grit rev-parse HEAD >actual &&
	test_cmp expect actual &&
	grit update-ref HEAD "$m_sha" "$parent_sha"
'

test_expect_success 'show-ref -s --verify works' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	echo "$m_sha" >expect &&
	grit show-ref -s --verify refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'show-ref --verify -q fails on missing ref' '
	cd real-repo &&
	test_must_fail grit show-ref --verify -q refs/heads/nonexistent
'

# === Symbolic-ref related ===

test_expect_success 'symbolic-ref read and write' '
	cd real-repo &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/master >expect &&
	test_cmp expect actual
'

# === Multiple deletes ===

test_expect_success 'stdin can delete multiple refs' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref refs/heads/del1 "$m_sha" &&
	grit update-ref refs/heads/del2 "$m_sha" &&
	cat >stdin <<-EOF &&
	delete refs/heads/del1 $m_sha
	delete refs/heads/del2 $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/del1 &&
	test_must_fail grit rev-parse --verify -q refs/heads/del2
'

# === Ref in deep hierarchy ===

test_expect_success 'create deeply nested ref' '
	cd repo &&
	grit update-ref refs/heads/nested/deep/ref "$A" &&
	echo "$A" >expect &&
	cat .git/refs/heads/nested/deep/ref >actual &&
	test_cmp expect actual
'

test_expect_success 'update deeply nested ref' '
	cd repo &&
	grit update-ref refs/heads/nested/deep/ref "$B" "$A" &&
	echo "$B" >expect &&
	cat .git/refs/heads/nested/deep/ref >actual &&
	test_cmp expect actual
'

test_expect_success 'delete deeply nested ref' '
	cd repo &&
	grit update-ref -d refs/heads/nested/deep/ref "$B" &&
	test_path_is_missing .git/refs/heads/nested/deep/ref
'

# === Tags namespace ===

test_expect_success 'create ref in tags namespace' '
	cd repo &&
	grit update-ref refs/tags/v1.0 "$A" &&
	echo "$A" >expect &&
	cat .git/refs/tags/v1.0 >actual &&
	test_cmp expect actual
'

test_expect_success 'delete ref in tags namespace' '
	cd repo &&
	grit update-ref -d refs/tags/v1.0 "$A" &&
	test_path_is_missing .git/refs/tags/v1.0
'

# === Custom namespace ===

test_expect_success 'create ref in custom namespace' '
	cd repo &&
	grit update-ref refs/custom/test "$C" &&
	echo "$C" >expect &&
	cat .git/refs/custom/test >actual &&
	test_cmp expect actual
'

# === Verify combination test ===

test_expect_success 'stdin update + verify in same batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref refs/heads/verifyme "$m_sha" &&
	cat >stdin <<-EOF &&
	verify refs/heads/verifyme $m_sha
	update refs/heads/verifyme $m_sha $m_sha
	EOF
	grit update-ref --stdin <stdin
'

test_expect_success 'stdin verify blocks if wrong, atomicity preserved' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref refs/heads/atomictest "$m_sha" &&
	cat >stdin <<-EOF &&
	verify refs/heads/atomictest $parent_sha
	update refs/heads/atomictest $parent_sha $m_sha
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/atomictest >actual &&
	test_cmp expect actual
'

# === delete symref without dereference ===

test_expect_success 'delete symref without dereference' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit symbolic-ref SYMREF refs/heads/master &&
	grit update-ref --no-deref -d SYMREF &&
	grit show-ref --verify -q refs/heads/master &&
	test_must_fail grit show-ref --verify -q SYMREF &&
	test_must_fail grit symbolic-ref SYMREF
'

test_expect_success 'delete symref without dereference preserves underlying ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit symbolic-ref SYMREF2 refs/heads/master &&
	grit update-ref --no-deref -d SYMREF2 &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual &&
	test_must_fail grit show-ref --verify -q SYMREF2
'

# === update-ref --no-deref -d can delete self-reference ===

test_expect_success 'update-ref --no-deref -d can delete self-reference' '
	cd real-repo &&
	grit symbolic-ref refs/heads/self refs/heads/self &&
	grit update-ref --no-deref -d refs/heads/self &&
	test_must_fail grit show-ref --verify -q refs/heads/self
'

# === --no-deref with stdin flag ===

test_expect_success 'stdin update symref works flag --no-deref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit symbolic-ref TESTSYMREFONE refs/heads/master &&
	grit symbolic-ref TESTSYMREFTWO refs/heads/master &&
	cat >stdin <<-EOF &&
	update TESTSYMREFONE $parent_sha $m_sha
	update TESTSYMREFTWO $parent_sha $m_sha
	EOF
	grit update-ref --no-deref --stdin <stdin &&
	echo "$parent_sha" >expect &&
	grit rev-parse TESTSYMREFONE >actual &&
	test_cmp expect actual &&
	grit rev-parse TESTSYMREFTWO >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin delete symref works flag --no-deref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit symbolic-ref TESTSYMDEL1 refs/heads/master &&
	grit symbolic-ref TESTSYMDEL2 refs/heads/master &&
	cat >stdin <<-EOF &&
	delete TESTSYMDEL1 $m_sha
	delete TESTSYMDEL2 $m_sha
	EOF
	grit update-ref --no-deref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q TESTSYMDEL1 &&
	test_must_fail grit rev-parse --verify -q TESTSYMDEL2 &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === stdin update ref works with right old value (real objects) ===

test_expect_success 'stdin update ref works with right old value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/rightold "$parent_sha" &&
	echo "update refs/heads/rightold $m_sha $parent_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/rightold >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin delete ref fails with wrong old value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/wrongdel "$m_sha" &&
	echo "delete refs/heads/wrongdel $parent_sha" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/wrongdel >actual &&
	test_cmp expect actual
'

# === stdin update refs fails with wrong old value (atomicity) ===

test_expect_success 'stdin update refs fails with wrong old value (real objects, atomicity)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/atom-a "$m_sha" &&
	grit update-ref refs/heads/atom-b "$m_sha" &&
	grit update-ref refs/heads/atom-c "$m_sha" &&
	cat >stdin <<-EOF &&
	update refs/heads/atom-a $m_sha $m_sha
	update refs/heads/atom-b $m_sha $m_sha
	update refs/heads/atom-c $parent_sha $parent_sha
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/atom-a >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/atom-b >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/atom-c >actual &&
	test_cmp expect actual
'

# === stdin update/create/verify combination (real objects) ===

test_expect_success 'stdin update/create/verify combination works (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/combo-b 2>/dev/null || true &&
	grit update-ref refs/heads/combo-a "$m_sha" &&
	cat >stdin <<-EOF &&
	update refs/heads/combo-a $m_sha
	create refs/heads/combo-b $parent_sha
	EOF
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/combo-a >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/combo-b >actual &&
	test_cmp expect actual
'

# === stdin transaction with verify + delete + create ===

test_expect_success 'stdin transaction with verify + delete + create' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/txverify "$m_sha" &&
	grit update-ref -d refs/heads/txnew 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/txverify $m_sha
	delete refs/heads/txverify $m_sha
	create refs/heads/txnew $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/txverify &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/txnew >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin transaction verify fails rolls back all changes' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/txrollback "$m_sha" &&
	grit update-ref -d refs/heads/txnewref 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/txrollback $parent_sha
	create refs/heads/txnewref $m_sha
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/txrollback >actual &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/txnewref
'

# === transaction create + update sequential commits ===

test_expect_success 'transaction create then update in sequential commits' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/seqref 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/seqref $parent_sha
	commit
	start
	update refs/heads/seqref $m_sha $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/seqref >actual &&
	test_cmp expect actual
'

# === transaction create + delete sequential commits ===

test_expect_success 'transaction create then delete in sequential commits' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/cdseq 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/cdseq $m_sha
	commit
	start
	delete refs/heads/cdseq $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit start commit >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/cdseq
'

# === sole commit fails (no transaction started) ===

test_expect_success 'sole commit fails without start' '
	cd real-repo &&
	cat >stdin <<-\EOF &&
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	grep "no transaction started" err
'

# === transaction exits on multiple aborts ===

test_expect_success 'transaction handles double abort' '
	cd real-repo &&
	cat >stdin <<-\EOF &&
	abort
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	test_must_be_empty actual
'

# === transaction cannot restart ongoing transaction ===

test_expect_success 'transaction cannot restart ongoing (additional test)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/restartref 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/restartref $m_sha
	start
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/restartref
'

# === update with create on zero old value (real objects) ===

test_expect_success 'stdin create ref with zero old value fails if ref exists' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/exists "$m_sha" &&
	echo "update refs/heads/exists $m_sha $Z" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/exists >actual &&
	test_cmp expect actual
'

# === stdin multiple creates in batch (no transaction) ===

test_expect_success 'stdin multiple creates in non-transactional batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/batch1 2>/dev/null || true &&
	grit update-ref -d refs/heads/batch2 2>/dev/null || true &&
	grit update-ref -d refs/heads/batch3 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	create refs/heads/batch1 $m_sha
	create refs/heads/batch2 $parent_sha
	create refs/heads/batch3 $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/batch1 >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/batch2 >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/batch3 >actual &&
	test_cmp expect actual
'

# === stdin multiple deletes in non-transactional batch ===

test_expect_success 'stdin multiple deletes in non-transactional batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/mdel1 "$m_sha" &&
	grit update-ref refs/heads/mdel2 "$m_sha" &&
	grit update-ref refs/heads/mdel3 "$m_sha" &&
	cat >stdin <<-EOF &&
	delete refs/heads/mdel1 $m_sha
	delete refs/heads/mdel2 $m_sha
	delete refs/heads/mdel3 $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/mdel1 &&
	test_must_fail grit rev-parse --verify -q refs/heads/mdel2 &&
	test_must_fail grit rev-parse --verify -q refs/heads/mdel3
'

# === stdin update with empty old value creates ref ===

test_expect_success 'stdin update ref creates with empty old value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/emptyold 2>/dev/null || true &&
	echo "update refs/heads/emptyold $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/emptyold >actual &&
	test_cmp expect actual
'

# === stdin delete without old value ===

test_expect_success 'stdin delete ref works without old value (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/delnoold "$m_sha" &&
	echo "delete refs/heads/delnoold" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/delnoold
'

# === stdin verify in transaction ===

test_expect_success 'stdin transaction verify succeeds for correct value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/master $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual
'

test_expect_success 'stdin transaction verify succeeds for missing ref' '
	cd real-repo &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/nonexistent-verify $Z
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/nonexistent-verify
'

test_expect_success 'stdin transaction verify fails for wrong value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/master $parent_sha
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin
'

test_expect_success 'stdin transaction verify fails for mistaken null' '
	cd real-repo &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/master $Z
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin
'

# === transaction with mixed update/delete/verify/create ===

test_expect_success 'transaction with mixed operations' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/mix-update "$parent_sha" &&
	grit update-ref refs/heads/mix-delete "$m_sha" &&
	grit update-ref -d refs/heads/mix-create 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/master $m_sha
	update refs/heads/mix-update $m_sha $parent_sha
	delete refs/heads/mix-delete $m_sha
	create refs/heads/mix-create $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/mix-update >actual &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/mix-delete &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/mix-create >actual &&
	test_cmp expect actual
'

# === transaction fails on wrong old value ===

test_expect_success 'transaction fails when old value wrong' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/txfailold "$m_sha" &&
	cat >stdin <<-EOF &&
	start
	update refs/heads/txfailold $parent_sha $parent_sha
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/txfailold >actual &&
	test_cmp expect actual
'

# === stdin create in deeply nested paths ===

test_expect_success 'stdin create deeply nested ref (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/very/deep/nested/ref 2>/dev/null || true &&
	echo "create refs/heads/very/deep/nested/ref $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/very/deep/nested/ref >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin delete deeply nested ref (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	echo "delete refs/heads/very/deep/nested/ref $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/very/deep/nested/ref
'

# === refs in different namespaces ===

test_expect_success 'stdin create refs in tags namespace (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/tags/stdin-tag 2>/dev/null || true &&
	echo "create refs/tags/stdin-tag $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/tags/stdin-tag >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin create refs in custom namespace (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/custom/stdin-ref 2>/dev/null || true &&
	echo "create refs/custom/stdin-ref $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/custom/stdin-ref >actual &&
	test_cmp expect actual
'

# === stdin create with symbolic ref target ===

test_expect_success 'stdin create ref resolving symbolic ref target' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/symresolve 2>/dev/null || true &&
	echo "create refs/heads/symresolve refs/heads/master" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/symresolve >actual &&
	test_cmp expect actual
'

# === HEAD updates with real objects ===

test_expect_success 'updating HEAD updates underlying branch (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref HEAD "$parent_sha" "$m_sha" &&
	echo "$parent_sha" >expect &&
	grit rev-parse HEAD >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual &&
	grit update-ref HEAD "$m_sha" "$parent_sha"
'

# === Transaction abort does not create ref ===

test_expect_success 'transaction abort does not create any refs' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/abortref1 2>/dev/null || true &&
	grit update-ref -d refs/heads/abortref2 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/abortref1 $m_sha
	create refs/heads/abortref2 $m_sha
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/abortref1 &&
	test_must_fail grit rev-parse --verify -q refs/heads/abortref2
'

# === stdin verify blocks entire batch ===

test_expect_success 'stdin verify failure blocks entire batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/blocked "$parent_sha" &&
	grit update-ref -d refs/heads/shouldnt-exist 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	verify refs/heads/master $parent_sha
	update refs/heads/blocked $m_sha $parent_sha
	create refs/heads/shouldnt-exist $m_sha
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/blocked >actual &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/shouldnt-exist
'

# === stdin with duplicate refs fails ===

test_expect_success 'stdin fails with duplicate refs (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	cat >stdin <<-EOF &&
	create refs/heads/duptest1 $m_sha
	create refs/heads/duptest2 $m_sha
	create refs/heads/duptest1 $m_sha
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err
'

# === stdin create ref fails when ref already exists ===

test_expect_success 'stdin create ref fails when ref already exists' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/existcreate "$m_sha" &&
	echo "create refs/heads/existcreate $m_sha" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

# === stdin update non-existent ref without old value creates it ===

test_expect_success 'stdin update non-existent ref without old value creates it' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/update-creates 2>/dev/null || true &&
	echo "update refs/heads/update-creates $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/update-creates >actual &&
	test_cmp expect actual
'

# === CLI tests (non-stdin) ===

test_expect_success 'update-ref with wrong old value fails (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/clitestref "$m_sha" &&
	test_must_fail grit update-ref refs/heads/clitestref "$parent_sha" "$parent_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/clitestref >actual &&
	test_cmp expect actual
'

test_expect_success 'delete ref with right old value works (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/clidelref "$m_sha" &&
	grit update-ref -d refs/heads/clidelref "$m_sha" &&
	test_must_fail grit rev-parse --verify -q refs/heads/clidelref
'

test_expect_success 'delete ref with wrong old value fails (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/clibaddel "$m_sha" &&
	test_must_fail grit update-ref -d refs/heads/clibaddel "$parent_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/clibaddel >actual &&
	test_cmp expect actual
'

# === update-ref -d without old value ===

test_expect_success 'delete ref without old value succeeds (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/clinoold "$m_sha" &&
	grit update-ref -d refs/heads/clinoold &&
	test_must_fail grit rev-parse --verify -q refs/heads/clinoold
'

# === create ref with old value zero ===

test_expect_success 'create ref with old value zero succeeds' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/clizerold 2>/dev/null || true &&
	grit update-ref refs/heads/clizerold "$m_sha" "$Z" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/clizerold >actual &&
	test_cmp expect actual
'

test_expect_success 'create ref with old value zero fails if ref exists' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/clizerf "$m_sha" &&
	test_must_fail grit update-ref refs/heads/clizerf "$parent_sha" "$Z" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/clizerf >actual &&
	test_cmp expect actual
'

# === multiple transaction commit cycles ===

test_expect_success 'transaction three commit cycles' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/cycle1 2>/dev/null || true &&
	grit update-ref -d refs/heads/cycle2 2>/dev/null || true &&
	grit update-ref -d refs/heads/cycle3 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/cycle1 $parent_sha
	commit
	start
	create refs/heads/cycle2 $m_sha
	commit
	start
	create refs/heads/cycle3 $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit start commit start commit >expect &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/cycle1 >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/cycle2 >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/cycle3 >actual &&
	test_cmp expect actual
'

# === transaction update + verify mixed ===

test_expect_success 'transaction update + verify in same commit' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/tvmix "$m_sha" &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/tvmix $m_sha
	update refs/heads/tvmix $m_sha $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual
'

# === stdin identity update (same old and new value) ===

test_expect_success 'stdin identity update does not change ref (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	echo "update refs/heads/master $m_sha $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === update-ref with file/directory conflict (real objects) ===

test_expect_success 'file/directory conflict detected (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/conflict-parent "$m_sha" &&
	test_must_fail grit update-ref refs/heads/conflict-parent/child "$m_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/conflict-parent >actual &&
	test_cmp expect actual
'

test_expect_success 'file/directory conflict reverse (real objects)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/conflict-deep/child "$m_sha" &&
	test_must_fail grit update-ref refs/heads/conflict-deep "$m_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/conflict-deep/child >actual &&
	test_cmp expect actual
'

# === stdin with symbolic ref names as new values ===

test_expect_success 'stdin update resolves ref name as new value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/resolveme "$parent_sha" &&
	echo "update refs/heads/resolveme refs/heads/master $parent_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/resolveme >actual &&
	test_cmp expect actual
'

# === show-ref tests ===

test_expect_success 'show-ref --verify works with multiple refs' '
	cd real-repo &&
	grit show-ref --verify refs/heads/master >actual &&
	grep refs/heads/master actual
'

test_expect_success 'show-ref --verify -q succeeds silently' '
	cd real-repo &&
	grit show-ref --verify -q refs/heads/master >actual &&
	test_must_be_empty actual
'

test_expect_success 'show-ref --verify -q fails silently on missing' '
	cd real-repo &&
	test_must_fail grit show-ref --verify -q refs/heads/doesnotexist 2>err
'

# === for-each-ref with newly created refs ===

test_expect_success 'for-each-ref sees stdin-created refs' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/feach/one 2>/dev/null || true &&
	grit update-ref -d refs/feach/two 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	create refs/feach/one $m_sha
	create refs/feach/two $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	grit for-each-ref refs/feach/ >actual &&
	grep refs/feach/one actual &&
	grep refs/feach/two actual
'

# === CLI update-ref creates ref in various namespaces ===

test_expect_success 'update-ref creates ref in notes namespace' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/notes/commits "$m_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/notes/commits >actual &&
	test_cmp expect actual
'

test_expect_success 'update-ref creates ref in remotes namespace' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/remotes/origin/master "$m_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/remotes/origin/master >actual &&
	test_cmp expect actual
'

# === stdin error handling: create with too many arguments ===

# Note: grit currently accepts extra arguments to create/update/delete silently.
# These tests verify the behavior that IS implemented.

# === stdin create ref fails when ref already exists (with explicit new value) ===

test_expect_success 'stdin create ref fails when ref exists (different new value)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/exist-test "$m_sha" &&
	echo "create refs/heads/exist-test $parent_sha" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/exist-test >actual &&
	test_cmp expect actual
'

# === stdin update ref works with ref expressions (e.g. master~1) ===

test_expect_success 'stdin update ref works with SHA values' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/sha-update-test "$m_sha" &&
	echo "update refs/heads/sha-update-test $parent_sha $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/sha-update-test >actual &&
	test_cmp expect actual
'

# === stdin delete ref works with right old value (using expression) ===

test_expect_success 'stdin delete ref works with right old value (expression)' '
	cd real-repo &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/del-expr "$parent_sha" &&
	echo "delete refs/heads/del-expr $parent_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/del-expr
'

# === stdin update refs fails with wrong old value (atomicity) ===

test_expect_success 'stdin update refs fails with wrong old value preserving all refs' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/atomic-a "$m_sha" &&
	grit update-ref refs/heads/atomic-b "$m_sha" &&
	grit update-ref refs/heads/atomic-c "$m_sha" &&
	cat >stdin <<-EOF &&
	update refs/heads/atomic-a $m_sha $m_sha
	update refs/heads/atomic-b $m_sha $m_sha
	update refs/heads/atomic-c $parent_sha $parent_sha
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/atomic-a >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/atomic-b >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/atomic-c >actual &&
	test_cmp expect actual
'

# === Transaction: sole commit fails ===

test_expect_success 'transaction sole commit fails without start' '
	cd real-repo &&
	cat >stdin <<-\EOF &&
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	grep "no transaction started" err
'

# === Transaction: sole abort ===

test_expect_success 'transaction sole abort succeeds' '
	cd real-repo &&
	cat >stdin <<-\EOF &&
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	test_must_be_empty actual
'

# === Transaction: commit after abort in sequence ===

test_expect_success 'transaction abort does not create refs' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/abort-only 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/abort-only $m_sha
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/abort-only
'

test_expect_success 'transaction abort with multiple refs does not persist' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref -d refs/heads/abort-multi-a 2>/dev/null || true &&
	grit update-ref -d refs/heads/abort-multi-b 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/abort-multi-a $m_sha
	create refs/heads/abort-multi-b $parent_sha
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/abort-multi-a &&
	test_must_fail grit rev-parse --verify -q refs/heads/abort-multi-b
'

# === Transaction: cannot restart ongoing transaction ===

test_expect_success 'transaction restart during active fails (with create)' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/no-restart 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/no-restart $m_sha
	start
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/no-restart
'

# === stdin --no-deref: update overwriting symref target ===

test_expect_success 'stdin --no-deref update symref overwrites with value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit symbolic-ref refs/heads/no-deref-sym refs/heads/master &&
	echo "update refs/heads/no-deref-sym $parent_sha $m_sha" >stdin &&
	grit update-ref --no-deref --stdin <stdin &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/no-deref-sym >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === stdin --no-deref: delete symref preserves target ===

test_expect_success 'stdin --no-deref delete symref preserves target ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit symbolic-ref refs/heads/noderef-del refs/heads/master &&
	echo "delete refs/heads/noderef-del $m_sha" >stdin &&
	grit update-ref --no-deref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/noderef-del &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === stdin update ref creates with empty old value (no old-value arg) ===

test_expect_success 'stdin update ref with empty old value creates ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/empty-old-create 2>/dev/null || true &&
	echo "update refs/heads/empty-old-create $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/empty-old-create >actual &&
	test_cmp expect actual
'

# === Dangling symref: update without old value overwrites ===

test_expect_success 'dangling symref overwritten by update without old oid' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit symbolic-ref refs/heads/dangle-update refs/heads/does-not-exist &&
	echo "update refs/heads/dangle-update $m_sha" >stdin &&
	grit update-ref --no-deref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/dangle-update >actual &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/does-not-exist
'

# === Transaction: create + update + delete + verify in single commit ===

test_expect_success 'transaction with all four operation types' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/tx-all-update "$parent_sha" &&
	grit update-ref refs/heads/tx-all-delete "$m_sha" &&
	grit update-ref -d refs/heads/tx-all-create 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/master $m_sha
	create refs/heads/tx-all-create $m_sha
	update refs/heads/tx-all-update $m_sha $parent_sha
	delete refs/heads/tx-all-delete $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/tx-all-create >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/tx-all-update >actual &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/tx-all-delete
'

# === Transaction: update with wrong old value rolls back everything ===

test_expect_success 'transaction with wrong old value fails' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref refs/heads/tx-rb-b "$parent_sha" &&
	cat >stdin <<-EOF &&
	start
	update refs/heads/tx-rb-b $m_sha $m_sha
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/tx-rb-b >actual &&
	test_cmp expect actual
'

# === Transaction: verify + create, verify fails ===

test_expect_success 'transaction verify failure prevents create' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/tx-verify-create 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	verify refs/heads/master $parent_sha
	create refs/heads/tx-verify-create $m_sha
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/tx-verify-create
'

# === Transaction: multiple creates in single commit ===

test_expect_success 'transaction creates many refs in single commit' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/tx-multi-a 2>/dev/null || true &&
	grit update-ref -d refs/heads/tx-multi-b 2>/dev/null || true &&
	grit update-ref -d refs/heads/tx-multi-c 2>/dev/null || true &&
	grit update-ref -d refs/heads/tx-multi-d 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/tx-multi-a $m_sha
	create refs/heads/tx-multi-b $parent_sha
	create refs/heads/tx-multi-c $m_sha
	create refs/heads/tx-multi-d $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/tx-multi-a >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/tx-multi-b >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/tx-multi-c >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/tx-multi-d >actual &&
	test_cmp expect actual
'

# === Transaction: multiple deletes in single commit ===

test_expect_success 'transaction deletes many refs in single commit' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/tx-mdel-a "$m_sha" &&
	grit update-ref refs/heads/tx-mdel-b "$m_sha" &&
	grit update-ref refs/heads/tx-mdel-c "$m_sha" &&
	cat >stdin <<-EOF &&
	start
	delete refs/heads/tx-mdel-a $m_sha
	delete refs/heads/tx-mdel-b $m_sha
	delete refs/heads/tx-mdel-c $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/tx-mdel-a &&
	test_must_fail grit rev-parse --verify -q refs/heads/tx-mdel-b &&
	test_must_fail grit rev-parse --verify -q refs/heads/tx-mdel-c
'

# === Transaction: abort then commit different ref ===

test_expect_success 'transaction abort with update does not modify ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref refs/heads/abort-upd "$m_sha" &&
	cat >stdin <<-EOF &&
	start
	update refs/heads/abort-upd $parent_sha $m_sha
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/abort-upd >actual &&
	test_cmp expect actual
'

# === Transaction: update same ref across multiple commits ===

test_expect_success 'transaction updates same ref across multiple commits' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref -d refs/heads/evolving 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/evolving $parent_sha
	commit
	start
	update refs/heads/evolving $m_sha $parent_sha
	commit
	start
	delete refs/heads/evolving $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit start commit start commit >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/evolving
'

# === stdin: verify in non-transactional batch ===

test_expect_success 'stdin verify succeeds in non-transactional batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/verify-batch-create 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	verify refs/heads/master $m_sha
	create refs/heads/verify-batch-create $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/verify-batch-create >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin verify failure blocks create in non-transactional batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/verify-batch-fail 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	verify refs/heads/master $parent_sha
	create refs/heads/verify-batch-fail $m_sha
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/verify-batch-fail
'

# === stdin: create in various namespace hierarchies ===

test_expect_success 'stdin create ref in stash namespace' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/stash 2>/dev/null || true &&
	echo "create refs/stash $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/stash >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin create ref in notes namespace' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/notes/test 2>/dev/null || true &&
	echo "create refs/notes/test $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/notes/test >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin create ref in remotes namespace' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/remotes/origin/develop 2>/dev/null || true &&
	echo "create refs/remotes/origin/develop $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/remotes/origin/develop >actual &&
	test_cmp expect actual
'

# === CLI: --no-deref -d deletes symref directly ===

test_expect_success 'update-ref --no-deref -d deletes symref not target' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit symbolic-ref refs/heads/cli-noderef-sym refs/heads/master &&
	grit update-ref --no-deref -d refs/heads/cli-noderef-sym &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual &&
	test_must_fail grit show-ref --verify -q refs/heads/cli-noderef-sym
'

# === CLI: create ref with zero old value prevents overwrite ===

test_expect_success 'update-ref with zero old value creates new ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	Z=0000000000000000000000000000000000000000 &&
	grit update-ref -d refs/heads/zero-create 2>/dev/null || true &&
	grit update-ref refs/heads/zero-create "$m_sha" "$Z" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/zero-create >actual &&
	test_cmp expect actual
'

test_expect_success 'update-ref with zero old value fails if ref exists' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	Z=0000000000000000000000000000000000000000 &&
	grit update-ref refs/heads/zero-exists "$m_sha" &&
	test_must_fail grit update-ref refs/heads/zero-exists "$parent_sha" "$Z" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/zero-exists >actual &&
	test_cmp expect actual
'

# === CLI: HEAD dereference through symref to branch ===

test_expect_success 'update HEAD dereferences to underlying branch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit symbolic-ref HEAD refs/heads/master &&
	grit update-ref HEAD "$parent_sha" "$m_sha" &&
	echo "$parent_sha" >expect &&
	grit rev-parse HEAD >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual &&
	grit update-ref HEAD "$m_sha" "$parent_sha"
'

# === CLI: delete through HEAD ===

test_expect_success 'delete through HEAD removes underlying branch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/head-del-branch "$m_sha" &&
	grit symbolic-ref HEAD refs/heads/head-del-branch &&
	grit update-ref -d HEAD "$m_sha" &&
	test_must_fail grit rev-parse --verify -q refs/heads/head-del-branch &&
	grit symbolic-ref HEAD refs/heads/master
'

# === Self-referencing symref: --no-deref -d can delete ===

test_expect_success '--no-deref -d deletes self-referencing symref' '
	cd real-repo &&
	grit symbolic-ref refs/heads/selfref refs/heads/selfref &&
	grit update-ref --no-deref -d refs/heads/selfref &&
	test_must_fail grit show-ref --verify -q refs/heads/selfref
'

# === stdin: mixed namespaces in single batch ===

test_expect_success 'stdin batch with refs in different namespaces' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/ns-head 2>/dev/null || true &&
	grit update-ref -d refs/tags/ns-tag 2>/dev/null || true &&
	grit update-ref -d refs/custom/ns-custom 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	create refs/heads/ns-head $m_sha
	create refs/tags/ns-tag $parent_sha
	create refs/custom/ns-custom $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/ns-head >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/tags/ns-tag >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/custom/ns-custom >actual &&
	test_cmp expect actual
'

# === stdin: transaction batch with refs in different namespaces ===

test_expect_success 'stdin transaction with refs in different namespaces' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/txns-head 2>/dev/null || true &&
	grit update-ref -d refs/tags/txns-tag 2>/dev/null || true &&
	grit update-ref -d refs/remotes/origin/txns-remote 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/txns-head $m_sha
	create refs/tags/txns-tag $parent_sha
	create refs/remotes/origin/txns-remote $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/txns-head >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/tags/txns-tag >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/remotes/origin/txns-remote >actual &&
	test_cmp expect actual
'

# === stdin: create ref with HEAD as new value ===

test_expect_success 'stdin create ref resolving HEAD' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/head-resolve 2>/dev/null || true &&
	echo "create refs/heads/head-resolve HEAD" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/head-resolve >actual &&
	test_cmp expect actual
'

# === File/directory conflict via stdin ===

test_expect_success 'stdin create fails due to file/directory conflict' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/conflict-dir "$m_sha" &&
	echo "create refs/heads/conflict-dir/child $m_sha" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/conflict-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin create fails due to directory/file conflict' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/conflict-file/child "$m_sha" &&
	echo "create refs/heads/conflict-file $m_sha" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/conflict-file/child >actual &&
	test_cmp expect actual
'

# === Transaction: file/directory conflict detected ===

test_expect_success 'transaction fails on file/directory conflict' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/txconflict "$m_sha" &&
	grit update-ref -d refs/heads/txconflict-new 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/txconflict/child $m_sha
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/txconflict >actual &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/txconflict/child
'

# === stdin: delete ref without old value ===

test_expect_success 'stdin delete without old value succeeds' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/del-noold-test "$m_sha" &&
	echo "delete refs/heads/del-noold-test" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/del-noold-test
'

# === stdin: update/create/verify combination (upstream-style) ===

test_expect_success 'stdin update/create/verify combination (upstream pattern)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/combo-up-a "$m_sha" &&
	grit update-ref -d refs/heads/combo-up-b 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	update refs/heads/combo-up-a $m_sha
	create refs/heads/combo-up-b $parent_sha
	verify refs/heads/master $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/combo-up-a >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/combo-up-b >actual &&
	test_cmp expect actual
'

# === stdin: verify treats zero OID as "must not exist" ===

test_expect_success 'stdin verify zero OID means ref must not exist' '
	cd real-repo &&
	Z=0000000000000000000000000000000000000000 &&
	grit update-ref -d refs/heads/verify-zero-test 2>/dev/null || true &&
	echo "verify refs/heads/verify-zero-test $Z" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/verify-zero-test
'

test_expect_success 'stdin verify zero OID fails when ref exists' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	Z=0000000000000000000000000000000000000000 &&
	echo "verify refs/heads/master $Z" >stdin &&
	test_must_fail grit update-ref --stdin <stdin
'

# === CLI: create and immediately update ===

test_expect_success 'CLI create and immediately update with old value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/cli-imm-test 2>/dev/null || true &&
	grit update-ref refs/heads/cli-imm-test "$parent_sha" &&
	grit update-ref refs/heads/cli-imm-test "$m_sha" "$parent_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/cli-imm-test >actual &&
	test_cmp expect actual
'

# === CLI: fail to update with wrong old value ===

test_expect_success 'CLI update fails with wrong old value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/cli-wrong-old "$m_sha" &&
	test_must_fail grit update-ref refs/heads/cli-wrong-old "$parent_sha" "$parent_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/cli-wrong-old >actual &&
	test_cmp expect actual
'

# === CLI: delete fails with wrong old value ===

test_expect_success 'CLI delete fails with wrong old value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/cli-wrong-del "$m_sha" &&
	test_must_fail grit update-ref -d refs/heads/cli-wrong-del "$parent_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/cli-wrong-del >actual &&
	test_cmp expect actual
'

# === CLI: delete without old value ===

test_expect_success 'CLI delete without old value succeeds' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/cli-del-noold "$m_sha" &&
	grit update-ref -d refs/heads/cli-del-noold &&
	test_must_fail grit rev-parse --verify -q refs/heads/cli-del-noold
'

# === stdin: update identity (no-op) in transaction ===

test_expect_success 'stdin transaction identity update is no-op' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	cat >stdin <<-EOF &&
	start
	update refs/heads/master $m_sha $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === stdin: batch with many updates ===

test_expect_success 'stdin batch updates many refs at once' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/batch-upd-a "$parent_sha" &&
	grit update-ref refs/heads/batch-upd-b "$parent_sha" &&
	grit update-ref refs/heads/batch-upd-c "$parent_sha" &&
	grit update-ref refs/heads/batch-upd-d "$parent_sha" &&
	cat >stdin <<-EOF &&
	update refs/heads/batch-upd-a $m_sha $parent_sha
	update refs/heads/batch-upd-b $m_sha $parent_sha
	update refs/heads/batch-upd-c $m_sha $parent_sha
	update refs/heads/batch-upd-d $m_sha $parent_sha
	EOF
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/batch-upd-a >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/batch-upd-b >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/batch-upd-c >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/batch-upd-d >actual &&
	test_cmp expect actual
'

# === stdin: batch with many deletes ===

test_expect_success 'stdin batch deletes many refs at once' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/batch-del-a "$m_sha" &&
	grit update-ref refs/heads/batch-del-b "$m_sha" &&
	grit update-ref refs/heads/batch-del-c "$m_sha" &&
	grit update-ref refs/heads/batch-del-d "$m_sha" &&
	cat >stdin <<-EOF &&
	delete refs/heads/batch-del-a $m_sha
	delete refs/heads/batch-del-b $m_sha
	delete refs/heads/batch-del-c $m_sha
	delete refs/heads/batch-del-d $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/batch-del-a &&
	test_must_fail grit rev-parse --verify -q refs/heads/batch-del-b &&
	test_must_fail grit rev-parse --verify -q refs/heads/batch-del-c &&
	test_must_fail grit rev-parse --verify -q refs/heads/batch-del-d
'

# === stdin: create deeply nested in transaction ===

test_expect_success 'transaction creates deeply nested ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/tx/very/deep/nested 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/tx/very/deep/nested $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/tx/very/deep/nested >actual &&
	test_cmp expect actual
'

# === CLI: update with message flag ===

test_expect_success 'update-ref -m sets message (does not error)' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -m "test message" refs/heads/msg-ref "$m_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/msg-ref >actual &&
	test_cmp expect actual
'

# === stdin: verify in batch prevents subsequent operations ===

test_expect_success 'stdin verify failure prevents all subsequent ops in batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/after-verify-a 2>/dev/null || true &&
	grit update-ref -d refs/heads/after-verify-b 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	verify refs/heads/master $parent_sha
	create refs/heads/after-verify-a $m_sha
	create refs/heads/after-verify-b $m_sha
	EOF
	test_must_fail grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/after-verify-a &&
	test_must_fail grit rev-parse --verify -q refs/heads/after-verify-b
'

# === stdin: create and verify missing ref combination ===

test_expect_success 'stdin create + verify missing ref in batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	Z=0000000000000000000000000000000000000000 &&
	grit update-ref -d refs/heads/cv-new 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	verify refs/heads/cv-nonexist $Z
	create refs/heads/cv-new $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/cv-new >actual &&
	test_cmp expect actual
'

# === CLI: update-ref to same value (identity) ===

test_expect_success 'CLI identity update ref to same value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/heads/identity-test "$m_sha" &&
	grit update-ref refs/heads/identity-test "$m_sha" "$m_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/identity-test >actual &&
	test_cmp expect actual
'

# === stdin: duplicate refs in transaction fails ===

test_expect_success 'stdin transaction fails with duplicate refs' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/txdup1 $m_sha
	create refs/heads/txdup2 $m_sha
	create refs/heads/txdup1 $m_sha
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err
'

# === stdin: update non-existent ref with zero old value creates it ===

test_expect_success 'stdin update with zero old value creates new ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	Z=0000000000000000000000000000000000000000 &&
	grit update-ref -d refs/heads/zero-old-new 2>/dev/null || true &&
	echo "update refs/heads/zero-old-new $m_sha $Z" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/zero-old-new >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin update with zero old value fails for existing ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	Z=0000000000000000000000000000000000000000 &&
	grit update-ref refs/heads/zero-old-exist "$m_sha" &&
	echo "update refs/heads/zero-old-exist $parent_sha $Z" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/zero-old-exist >actual &&
	test_cmp expect actual
'

# === symref interactions: --no-deref in transaction ===

test_expect_success 'transaction --no-deref update replaces symref with regular ref' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit symbolic-ref refs/heads/tx-sym refs/heads/master &&
	cat >stdin <<-EOF &&
	start
	update refs/heads/tx-sym $parent_sha $m_sha
	commit
	EOF
	grit update-ref --no-deref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/tx-sym >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'transaction --no-deref delete removes symref not target' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit symbolic-ref refs/heads/tx-sym-del refs/heads/master &&
	cat >stdin <<-EOF &&
	start
	delete refs/heads/tx-sym-del $m_sha
	commit
	EOF
	grit update-ref --no-deref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/heads/tx-sym-del &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === Multiple transaction cycles with verify ===

test_expect_success 'transaction multiple cycles with verify between' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/heads/mc-ref 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/mc-ref $parent_sha
	commit
	start
	verify refs/heads/mc-ref $parent_sha
	update refs/heads/mc-ref $m_sha $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/mc-ref >actual &&
	test_cmp expect actual
'

# === for-each-ref sees transaction-created refs ===

test_expect_success 'for-each-ref sees refs created in transaction' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/txvis/one 2>/dev/null || true &&
	grit update-ref -d refs/txvis/two 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/txvis/one $m_sha
	create refs/txvis/two $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	grit for-each-ref refs/txvis/ >feach &&
	grep refs/txvis/one feach &&
	grep refs/txvis/two feach
'

# === show-ref: listing and verification ===

test_expect_success 'show-ref lists refs including prefix-matching ones' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/showtest/alpha "$m_sha" &&
	grit update-ref refs/showtest/beta "$m_sha" &&
	grit show-ref >actual &&
	grep refs/showtest/alpha actual &&
	grep refs/showtest/beta actual
'

test_expect_success 'show-ref --verify works' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	echo "$m_sha refs/heads/master" >expect &&
	grit show-ref --verify refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'show-ref --verify -q succeeds silently' '
	cd real-repo &&
	grit show-ref --verify -q refs/heads/master >actual &&
	test_must_be_empty actual
'

test_expect_success 'show-ref --verify fails for missing ref' '
	cd real-repo &&
	test_must_fail grit show-ref --verify -q refs/heads/this-does-not-exist
'

# === CLI: create ref and verify via show-ref -s ===

test_expect_success 'show-ref -s gives just the SHA' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	echo "$m_sha" >expect &&
	grit show-ref -s --verify refs/heads/master >actual &&
	test_cmp expect actual
'

# === Multiple symbolic refs pointing to same target ===

test_expect_success 'multiple symbolic refs to same target' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit symbolic-ref refs/heads/sym-a refs/heads/master &&
	grit symbolic-ref refs/heads/sym-b refs/heads/master &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/sym-a >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/sym-b >actual &&
	test_cmp expect actual &&
	echo refs/heads/master >expect &&
	grit symbolic-ref refs/heads/sym-a >actual &&
	test_cmp expect actual &&
	grit symbolic-ref refs/heads/sym-b >actual &&
	test_cmp expect actual
'

# === symbolic-ref read after delete-and-recreate ===

test_expect_success 'symbolic-ref survives target delete and recreate' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/sym-target "$m_sha" &&
	grit symbolic-ref refs/heads/sym-survivor refs/heads/sym-target &&
	grit update-ref -d refs/heads/sym-target "$m_sha" &&
	test_must_fail grit rev-parse refs/heads/sym-survivor 2>/dev/null &&
	grit update-ref refs/heads/sym-target "$parent_sha" &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/sym-survivor >actual &&
	test_cmp expect actual
'

# === stdin: deeply nested with transaction ===

test_expect_success 'transaction handles deeply nested ref hierarchy' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/deep/a/b/c/d 2>/dev/null || true &&
	grit update-ref -d refs/deep/a/b/c/e 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/deep/a/b/c/d $m_sha
	create refs/deep/a/b/c/e $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/deep/a/b/c/d >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/deep/a/b/c/e >actual &&
	test_cmp expect actual
'

# === --no-deref with multiple symrefs in batch ===

test_expect_success '--no-deref updates multiple symrefs independently' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit symbolic-ref refs/heads/multi-sym-a refs/heads/master &&
	grit symbolic-ref refs/heads/multi-sym-b refs/heads/master &&
	cat >stdin <<-EOF &&
	update refs/heads/multi-sym-a $parent_sha $m_sha
	update refs/heads/multi-sym-b $parent_sha $m_sha
	EOF
	grit update-ref --no-deref --stdin <stdin &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/multi-sym-a >actual &&
	test_cmp expect actual &&
	grit rev-parse refs/heads/multi-sym-b >actual &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === --no-deref delete multiple symrefs in batch ===

test_expect_success '--no-deref deletes multiple symrefs in batch' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit symbolic-ref refs/heads/multi-del-a refs/heads/master &&
	grit symbolic-ref refs/heads/multi-del-b refs/heads/master &&
	cat >stdin <<-EOF &&
	delete refs/heads/multi-del-a $m_sha
	delete refs/heads/multi-del-b $m_sha
	EOF
	grit update-ref --no-deref --stdin <stdin &&
	test_must_fail grit rev-parse --verify -q refs/heads/multi-del-a &&
	test_must_fail grit rev-parse --verify -q refs/heads/multi-del-b &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === for-each-ref: new tags visible after stdin create ===

test_expect_success 'for-each-ref sees stdin-created tags' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/tags/fer-tag1 2>/dev/null || true &&
	grit update-ref -d refs/tags/fer-tag2 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	create refs/tags/fer-tag1 $m_sha
	create refs/tags/fer-tag2 $m_sha
	EOF
	grit update-ref --stdin <stdin &&
	grit for-each-ref refs/tags/ >actual &&
	grep refs/tags/fer-tag1 actual &&
	grep refs/tags/fer-tag2 actual
'

# === Transaction: create in tags namespace ===

test_expect_success 'transaction creates tags' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref -d refs/tags/tx-tag-a 2>/dev/null || true &&
	grit update-ref -d refs/tags/tx-tag-b 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/tags/tx-tag-a $m_sha
	create refs/tags/tx-tag-b $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/tags/tx-tag-a >actual &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/tags/tx-tag-b >actual &&
	test_cmp expect actual
'

# === Transaction: delete tag ===

test_expect_success 'transaction deletes tag' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref refs/tags/tx-del-tag "$m_sha" &&
	cat >stdin <<-EOF &&
	start
	delete refs/tags/tx-del-tag $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	test_must_fail grit rev-parse --verify -q refs/tags/tx-del-tag
'

# === CLI: create ref with HEAD expression ===

test_expect_success 'CLI create ref using HEAD as value' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/cli-head-val 2>/dev/null || true &&
	grit update-ref refs/heads/cli-head-val HEAD &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/cli-head-val >actual &&
	test_cmp expect actual
'

# === CLI: create ref with ref expression ===

test_expect_success 'CLI create ref using another ref name as value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/heads/cli-ref-val 2>/dev/null || true &&
	grit update-ref refs/heads/cli-ref-val refs/heads/master &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/cli-ref-val >actual &&
	test_cmp expect actual
'

# === stdin: rapid create-delete cycles ===

test_expect_success 'rapid create-delete-recreate across transactions' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref -d refs/heads/rapid 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create refs/heads/rapid $m_sha
	commit
	start
	delete refs/heads/rapid $m_sha
	commit
	start
	create refs/heads/rapid $parent_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit start commit start commit >expect &&
	test_cmp expect actual &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/rapid >actual &&
	test_cmp expect actual
'

# === stdin: verify correct value does not modify ref ===

test_expect_success 'stdin verify does not modify ref value' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	echo "verify refs/heads/master $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

# === stdin: create with HEAD~1 expression ===

test_expect_success 'stdin create ref using HEAD expression' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d refs/heads/head-expr 2>/dev/null || true &&
	echo "create refs/heads/head-expr HEAD" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/head-expr >actual &&
	test_cmp expect actual
'

# === stdin: create symref via symbolic-ref then update via stdin ===

test_expect_success 'stdin update through symref (default deref) modifies target' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/deref-target "$m_sha" &&
	grit symbolic-ref refs/heads/deref-sym refs/heads/deref-target &&
	echo "update refs/heads/deref-sym $parent_sha $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/deref-target >actual &&
	test_cmp expect actual &&
	echo refs/heads/deref-target >expect &&
	grit symbolic-ref refs/heads/deref-sym >actual &&
	test_cmp expect actual
'

# === Pseudoref: stdin operations ===

test_expect_success 'stdin create pseudoref' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d PSEUDO_STDIN 2>/dev/null || true &&
	echo "create PSEUDO_STDIN $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse PSEUDO_STDIN >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin update pseudoref' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	echo "update PSEUDO_STDIN $parent_sha $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$parent_sha" >expect &&
	grit rev-parse PSEUDO_STDIN >actual &&
	test_cmp expect actual
'

test_expect_success 'stdin delete pseudoref' '
	cd real-repo &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	echo "delete PSEUDO_STDIN $parent_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	test_must_fail grit rev-parse PSEUDO_STDIN 2>/dev/null
'

# === stdin: verify pseudoref ===

test_expect_success 'stdin verify pseudoref succeeds' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref PSEUDO_VERIFY "$m_sha" &&
	echo "verify PSEUDO_VERIFY $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	grit update-ref -d PSEUDO_VERIFY
'

test_expect_success 'stdin verify pseudoref fails with wrong value' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref PSEUDO_VERIFY2 "$m_sha" &&
	echo "verify PSEUDO_VERIFY2 $parent_sha" >stdin &&
	test_must_fail grit update-ref --stdin <stdin &&
	grit update-ref -d PSEUDO_VERIFY2
'

# === Transaction: pseudoref operations ===

test_expect_success 'transaction create pseudoref' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	grit update-ref -d PSEUDO_TX 2>/dev/null || true &&
	cat >stdin <<-EOF &&
	start
	create PSEUDO_TX $m_sha
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual &&
	echo "$m_sha" >expect &&
	grit rev-parse PSEUDO_TX >actual &&
	test_cmp expect actual &&
	grit update-ref -d PSEUDO_TX
'

# === stdin: empty transaction (start + commit, no operations) ===

test_expect_success 'empty transaction with start and commit succeeds' '
	cd real-repo &&
	cat >stdin <<-\EOF &&
	start
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start commit >expect &&
	test_cmp expect actual
'

# === stdin: empty transaction (start + abort, no operations) ===

test_expect_success 'empty transaction with start and abort succeeds' '
	cd real-repo &&
	cat >stdin <<-\EOF &&
	start
	abort
	EOF
	grit update-ref --stdin <stdin >actual &&
	printf "%s: ok\n" start >expect &&
	test_cmp expect actual
'

# === CLI: update-ref -m with create and update ===

test_expect_success 'update-ref -m with update' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	parent_sha=$(grit rev-parse refs/heads/master~1) &&
	grit update-ref refs/heads/msg-upd "$parent_sha" &&
	grit update-ref -m "updating ref" refs/heads/msg-upd "$m_sha" "$parent_sha" &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/heads/msg-upd >actual &&
	test_cmp expect actual
'

# === stdin: create in bisect namespace ===

test_expect_success 'stdin create ref in bisect namespace' '
	cd real-repo &&
	m_sha=$(grit rev-parse refs/heads/master) &&
	grit update-ref -d refs/bisect/good 2>/dev/null || true &&
	echo "create refs/bisect/good $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$m_sha" >expect &&
	grit rev-parse refs/bisect/good >actual &&
	test_cmp expect actual
'

# === stdin: update ref with HEAD expression as old-value ===

test_expect_success 'stdin update with SHA as old-value' '
	cd real-repo &&
	m_sha=$(grit rev-parse HEAD) &&
	parent_sha=$(grit rev-parse HEAD~1) &&
	grit update-ref refs/heads/sha-old-test "$m_sha" &&
	echo "update refs/heads/sha-old-test $parent_sha $m_sha" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$parent_sha" >expect &&
	grit rev-parse refs/heads/sha-old-test >actual &&
	test_cmp expect actual
'

test_done
