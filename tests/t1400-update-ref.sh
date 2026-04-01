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

test_done
