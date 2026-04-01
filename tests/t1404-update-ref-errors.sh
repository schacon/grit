#!/bin/sh
# Ported from git/t/t1404-update-ref-errors.sh (harness-compatible subset).

test_description='grit update-ref error handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

C=3333333333333333333333333333333333333333
D=4444444444444444444444444444444444444444
E=5555555555555555555555555555555555555555

test_expect_success 'setup repository' '
	grit init repo &&
	cd repo
'

# === D/F (directory/file) conflict tests ===

test_expect_success 'existing loose ref blocks creating deeper ref' '
	cd repo &&
	grit update-ref refs/errors/c "$C" &&
	test_must_fail grit update-ref refs/errors/c/x "$D" &&
	echo "$C" >expect &&
	cat .git/refs/errors/c >actual &&
	test_cmp expect actual
'

test_expect_success 'existing deeper ref blocks creating parent ref' '
	cd repo &&
	grit update-ref refs/errors/d/e "$C" &&
	test_must_fail grit update-ref refs/errors/d "$D" &&
	echo "$C" >expect &&
	cat .git/refs/errors/d/e >actual &&
	test_cmp expect actual
'

test_expect_success 'existing loose ref is a deeper prefix of new' '
	cd repo &&
	grit update-ref refs/deep/c "$C" &&
	test_must_fail grit update-ref refs/deep/c/x/y "$D" &&
	echo "$C" >expect &&
	cat .git/refs/deep/c >actual &&
	test_cmp expect actual
'

test_expect_success 'existing deeper ref blocks creating shallow ref' '
	cd repo &&
	grit update-ref refs/deep2/c/x/y "$C" &&
	test_must_fail grit update-ref refs/deep2/c "$D" &&
	echo "$C" >expect &&
	cat .git/refs/deep2/c/x/y >actual &&
	test_cmp expect actual
'

# === --stdin mode error handling ===

test_expect_success 'missing old-value blocks update in --stdin mode' '
	cd repo &&
	echo "update refs/errors/missing $E $D" >stdin &&
	test_must_fail grit update-ref --stdin <stdin &&
	test_path_is_missing .git/refs/errors/missing
'

test_expect_success 'incorrect old-value blocks update in --stdin mode' '
	cd repo &&
	grit update-ref refs/errors/existing "$C" &&
	echo "update refs/errors/existing $E $D" >stdin &&
	test_must_fail grit update-ref --stdin <stdin &&
	echo "$C" >expect &&
	cat .git/refs/errors/existing >actual &&
	test_cmp expect actual
'

test_expect_success 'existing ref blocks create in --stdin mode' '
	cd repo &&
	echo "create refs/errors/existing $E" >stdin &&
	test_must_fail grit update-ref --stdin <stdin &&
	echo "$C" >expect &&
	cat .git/refs/errors/existing >actual &&
	test_cmp expect actual
'

test_expect_success 'incorrect old-value blocks delete in --stdin mode' '
	cd repo &&
	echo "delete refs/errors/existing $D" >stdin &&
	test_must_fail grit update-ref --stdin <stdin &&
	echo "$C" >expect &&
	cat .git/refs/errors/existing >actual &&
	test_cmp expect actual
'

# === Error handling with real objects ===

test_expect_success 'setup real repo for error tests' '
	grit init errrepo &&
	cd errrepo &&
	echo a > file && grit add file &&
	GIT_AUTHOR_NAME="Test" GIT_AUTHOR_EMAIL="t@t" \
	GIT_COMMITTER_NAME="Test" GIT_COMMITTER_EMAIL="t@t" \
	grit commit -m "first" &&
	echo b > file && grit add file &&
	GIT_AUTHOR_NAME="Test" GIT_AUTHOR_EMAIL="t@t" \
	GIT_COMMITTER_NAME="Test" GIT_COMMITTER_EMAIL="t@t" \
	grit commit -m "second" &&
	echo c > file && grit add file &&
	GIT_AUTHOR_NAME="Test" GIT_AUTHOR_EMAIL="t@t" \
	GIT_COMMITTER_NAME="Test" GIT_COMMITTER_EMAIL="t@t" \
	grit commit -m "third"
'

test_expect_success 'missing old value blocks update (real objects)' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	RE=$(grit rev-parse HEAD) &&
	printf "update refs/missing-update/foo $RE $RD\n" |
	test_must_fail grit update-ref --stdin 2>output.err &&
	grep "refs/missing-update/foo" output.err
'

test_expect_success 'incorrect old value blocks update (real objects)' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	RE=$(grit rev-parse HEAD) &&
	grit update-ref refs/incorrect-update/foo "$RC" &&
	printf "update refs/incorrect-update/foo $RE $RD\n" |
	test_must_fail grit update-ref --stdin 2>output.err &&
	grep "refs/incorrect-update/foo" output.err
'

test_expect_success 'existing old value blocks create (real objects)' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RE=$(grit rev-parse HEAD) &&
	grit update-ref refs/existing-create/foo "$RC" &&
	printf "create refs/existing-create/foo $RE\n" |
	test_must_fail grit update-ref --stdin 2>output.err &&
	grep "refs/existing-create/foo" output.err
'

test_expect_success 'incorrect old value blocks delete (real objects)' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	grit update-ref refs/incorrect-delete/foo "$RC" &&
	printf "delete refs/incorrect-delete/foo $RD\n" |
	test_must_fail grit update-ref --stdin 2>output.err &&
	grep "refs/incorrect-delete/foo" output.err
'

# === Indirect (via symbolic ref) error handling ===

test_expect_success 'missing old value blocks indirect update' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	RE=$(grit rev-parse HEAD) &&
	grit symbolic-ref refs/missing-indirect-update/symref refs/missing-indirect-update/foo &&
	printf "update refs/missing-indirect-update/symref $RE $RD\n" |
	test_must_fail grit update-ref --stdin 2>output.err &&
	grep "refs/missing-indirect-update" output.err
'

test_expect_success 'incorrect old value blocks indirect update' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	RE=$(grit rev-parse HEAD) &&
	grit symbolic-ref refs/incorrect-indirect-update/symref refs/incorrect-indirect-update/foo &&
	grit update-ref refs/incorrect-indirect-update/foo "$RC" &&
	printf "update refs/incorrect-indirect-update/symref $RE $RD\n" |
	test_must_fail grit update-ref --stdin 2>output.err &&
	grep "refs/incorrect-indirect-update" output.err
'

test_expect_success 'existing old value blocks indirect create' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RE=$(grit rev-parse HEAD) &&
	grit symbolic-ref refs/existing-indirect-create/symref refs/existing-indirect-create/foo &&
	grit update-ref refs/existing-indirect-create/foo "$RC" &&
	printf "create refs/existing-indirect-create/symref $RE\n" |
	test_must_fail grit update-ref --stdin 2>output.err &&
	grep "refs/existing-indirect-create" output.err
'

test_expect_success 'incorrect old value blocks indirect delete' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	grit symbolic-ref refs/incorrect-indirect-delete/symref refs/incorrect-indirect-delete/foo &&
	grit update-ref refs/incorrect-indirect-delete/foo "$RC" &&
	printf "delete refs/incorrect-indirect-delete/symref $RD\n" |
	test_must_fail grit update-ref --stdin 2>output.err &&
	grep "refs/incorrect-indirect-delete" output.err
'

# === D/F conflicts with real objects ===

test_expect_success 'D/F conflict: existing simple prefix blocks deeper (real objects)' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	grit update-ref refs/df1/a "$RC" &&
	grit update-ref refs/df1/c "$RC" &&
	grit update-ref refs/df1/e "$RC" &&
	test_must_fail grit update-ref refs/df1/c/x "$RC"
'

test_expect_success 'D/F conflict: existing deeper ref blocks simple prefix (real objects)' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	grit update-ref refs/df2/c/x "$RC" &&
	test_must_fail grit update-ref refs/df2/c "$RC"
'

test_expect_success 'D/F conflict: deeper prefix blocks even deeper (real objects)' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	grit update-ref refs/df3/c "$RC" &&
	test_must_fail grit update-ref refs/df3/c/x/y "$RC"
'

test_expect_success 'D/F conflict: deep ref blocks shallow ref (real objects)' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	grit update-ref refs/df4/c/x/y "$RC" &&
	test_must_fail grit update-ref refs/df4/c "$RC"
'

# === Stdin D/F conflict in batch ===

test_expect_success 'D/F conflict in stdin batch fails' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	grit update-ref refs/df5/a "$RC" &&
	cat >stdin <<-EOF &&
	create refs/df5/a/child $RC
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	grep -i "directory\|not a directory\|conflict" err
'

# === Transaction atomicity for errors ===

test_expect_success 'wrong old-value in transaction blocks commit' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	RE=$(grit rev-parse HEAD) &&
	grit update-ref refs/atom/a "$RC" &&
	cat >stdin <<-EOF &&
	start
	update refs/atom/a $RE $RD
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$RC" >expect &&
	grit rev-parse refs/atom/a >actual &&
	test_cmp expect actual
'

test_expect_success 'create existing ref in transaction fails' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RE=$(grit rev-parse HEAD) &&
	grit update-ref refs/atom/b "$RC" &&
	cat >stdin <<-EOF &&
	start
	create refs/atom/b $RE
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$RC" >expect &&
	grit rev-parse refs/atom/b >actual &&
	test_cmp expect actual
'

test_expect_success 'delete with wrong old-value in transaction fails' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	grit update-ref refs/atom/c "$RC" &&
	cat >stdin <<-EOF &&
	start
	delete refs/atom/c $RD
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$RC" >expect &&
	grit rev-parse refs/atom/c >actual &&
	test_cmp expect actual
'

# === Verify failure in transaction ===

test_expect_success 'verify failure blocks transaction' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	RD=$(grit rev-parse HEAD~1) &&
	RE=$(grit rev-parse HEAD) &&
	grit update-ref refs/atom/d "$RC" &&
	cat >stdin <<-EOF &&
	start
	verify refs/atom/d $RD
	update refs/atom/d $RE $RC
	commit
	EOF
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	echo "$RC" >expect &&
	grit rev-parse refs/atom/d >actual &&
	test_cmp expect actual
'

# === Mixed operations with errors ===

test_expect_success 'bad OID in create is rejected' '
	cd errrepo &&
	echo "create refs/bad/oid does-not-exist" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	grep "does-not-exist" err &&
	test_must_fail grit rev-parse --verify -q refs/bad/oid
'

test_expect_success 'bad OID in update old-value is rejected' '
	cd errrepo &&
	echo "update refs/bad/old $C does-not-exist" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err &&
	grep "does-not-exist" err
'

test_expect_success 'update non-existent ref without old-value succeeds' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	grit update-ref -d refs/noexist/ref 2>/dev/null || true &&
	echo "update refs/noexist/ref $RC" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$RC" >expect &&
	grit rev-parse refs/noexist/ref >actual &&
	test_cmp expect actual
'

test_expect_success 'delete non-existent ref fails with old-value' '
	cd errrepo &&
	RC=$(grit rev-parse HEAD~2) &&
	echo "delete refs/nonexist/del $RC" >stdin &&
	test_must_fail grit update-ref --stdin <stdin 2>err
'

test_expect_success 'delete non-existent ref without old-value succeeds silently' '
	cd errrepo &&
	grit update-ref -d refs/nonexist/del2 2>/dev/null || true &&
	echo "delete refs/nonexist/del2" >stdin &&
	grit update-ref --stdin <stdin
'

test_done
