#!/bin/sh

test_description='grit patch-id'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

OID_REGEX='[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]'

test_expect_success 'setup' '
	git init repo &&
	git -C repo config user.email "test@example.com" &&
	git -C repo config user.name "Test User" &&
	cd repo &&
	echo "line1" >file.txt &&
	git add file.txt &&
	test_tick &&
	git commit -m "initial" &&
	echo "line2" >>file.txt &&
	test_tick &&
	git commit -a -m "second" &&
	git branch same HEAD &&
	git commit --amend -m "second-amended" &&
	git checkout -b different HEAD~1 &&
	echo "different content" >file.txt &&
	test_tick &&
	git commit -a -m "different" &&
	git checkout master
'

test_expect_success 'patch-id output is well-formed' '
	cd repo &&
	git show HEAD >show.out &&
	git patch-id <show.out >output &&
	grep "^$OID_REGEX $OID_REGEX$" output
'

test_expect_success 'patch-id output contains the commit id' '
	cd repo &&
	HEAD=$(git rev-parse HEAD) &&
	git show HEAD >show.out &&
	git patch-id <show.out >output &&
	grep " $HEAD$" output
'

test_expect_success 'patch-id detects equality (same diff, different commit)' '
	cd repo &&
	git show master >show_main.out &&
	git show same >show_same.out &&
	git patch-id <show_main.out >pid_main.out &&
	git patch-id <show_same.out >pid_same.out &&
	sed "s/ .*//" pid_main.out >pid_main.id &&
	sed "s/ .*//" pid_same.out >pid_same.id &&
	test_cmp pid_main.id pid_same.id
'

test_expect_success 'patch-id detects inequality (different diff)' '
	cd repo &&
	git show master >show_main.out &&
	git show different >show_diff.out &&
	git patch-id <show_main.out >pid_main.out &&
	git patch-id <show_diff.out >pid_diff.out &&
	sed "s/ .*//" pid_main.out >pid_main.id &&
	sed "s/ .*//" pid_diff.out >pid_diff.id &&
	! test_cmp pid_main.id pid_diff.id
'

test_expect_success 'patch-id handles no-nl-at-eof markers' '
	cat >nonl.diff <<-\EOF &&
	commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
	diff --git i/a w/a
	index e69de29..2e65efe 100644
	--- i/a
	+++ w/a
	@@ -0,0 +1 @@
	+a
	\ No newline at end of file
	EOF
	cat >withnl.diff <<-\EOF &&
	commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
	diff --git i/a w/a
	index e69de29..7898192 100644
	--- i/a
	+++ w/a
	@@ -0,0 +1 @@
	+a
	EOF
	git patch-id <nonl.diff >nonl.out &&
	git patch-id <withnl.diff >withnl.out &&
	sed "s/ .*//" nonl.out >nonl.id &&
	sed "s/ .*//" withnl.out >withnl.id &&
	test_cmp nonl.id withnl.id
'

test_expect_success 'patch-id --stable and --unstable are accepted' '
	cd repo &&
	git show HEAD >show.out &&
	git patch-id --stable <show.out >out.stable &&
	git patch-id --unstable <show.out >out.unstable &&
	grep "^$OID_REGEX $OID_REGEX$" out.stable &&
	grep "^$OID_REGEX $OID_REGEX$" out.unstable
'

test_expect_success 'patch-id stable: file order irrelevant' '
	cat >foo-then-bar.diff <<-\EOF &&
	commit bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
	diff --git a/foo b/foo
	index e69de29..b14df8a 100644
	--- a/foo
	+++ b/foo
	@@ -0,0 +1 @@
	+hello
	diff --git a/bar b/bar
	index e69de29..b14df8a 100644
	--- a/bar
	+++ b/bar
	@@ -0,0 +1 @@
	+hello
	EOF
	cat >bar-then-foo.diff <<-\EOF &&
	commit bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
	diff --git a/bar b/bar
	index e69de29..b14df8a 100644
	--- a/bar
	+++ b/bar
	@@ -0,0 +1 @@
	+hello
	diff --git a/foo b/foo
	index e69de29..b14df8a 100644
	--- a/foo
	+++ b/foo
	@@ -0,0 +1 @@
	+hello
	EOF
	git patch-id --stable <foo-then-bar.diff >pid1.out &&
	git patch-id --stable <bar-then-foo.diff >pid2.out &&
	sed "s/ .*//" pid1.out >pid1.id &&
	sed "s/ .*//" pid2.out >pid2.id &&
	test_cmp pid1.id pid2.id
'

test_expect_success 'patch-id unstable: file order is relevant' '
	git patch-id --unstable <foo-then-bar.diff >pid1u.out &&
	git patch-id --unstable <bar-then-foo.diff >pid2u.out &&
	sed "s/ .*//" pid1u.out >pid1u.id &&
	sed "s/ .*//" pid2u.out >pid2u.id &&
	! test_cmp pid1u.id pid2u.id
'

# ---- more patch-id tests ----

test_expect_success 'patch-id --verbatim is accepted' '
	cd repo &&
	git show HEAD >show.out &&
	git patch-id --verbatim <show.out >out.verbatim &&
	grep "^$OID_REGEX $OID_REGEX$" out.verbatim
'

test_expect_success 'patch-id --verbatim implies stable (file order irrelevant)' '
	git patch-id --verbatim <foo-then-bar.diff >vpid1.out &&
	git patch-id --verbatim <bar-then-foo.diff >vpid2.out &&
	sed "s/ .*//" vpid1.out >vpid1.id &&
	sed "s/ .*//" vpid2.out >vpid2.id &&
	test_cmp vpid1.id vpid2.id
'

test_expect_success 'patch-id with whitespace-only change (verbatim vs default)' '
	cat >ws1.diff <<-\EOF &&
	commit cccccccccccccccccccccccccccccccccccccccc
	diff --git a/file b/file
	index e69de29..1234567 100644
	--- a/file
	+++ b/file
	@@ -0,0 +1 @@
	+hello world
	EOF
	cat >ws2.diff <<-\EOF &&
	commit cccccccccccccccccccccccccccccccccccccccc
	diff --git a/file b/file
	index e69de29..1234567 100644
	--- a/file
	+++ b/file
	@@ -0,0 +1 @@
	+hello  world
	EOF
	git patch-id <ws1.diff >ws1.out &&
	git patch-id <ws2.diff >ws2.out &&
	sed "s/ .*//" ws1.out >ws1.id &&
	sed "s/ .*//" ws2.out >ws2.id &&
	test_cmp ws1.id ws2.id
'

test_expect_success 'patch-id --verbatim distinguishes whitespace changes' '
	git patch-id --verbatim <ws1.diff >ws1v.out &&
	git patch-id --verbatim <ws2.diff >ws2v.out &&
	sed "s/ .*//" ws1v.out >ws1v.id &&
	sed "s/ .*//" ws2v.out >ws2v.id &&
	! test_cmp ws1v.id ws2v.id
'

test_expect_success 'patch-id with empty diff produces no output' '
	cat >empty.diff <<-\EOF &&
	commit dddddddddddddddddddddddddddddddddddddddd
	EOF
	git patch-id <empty.diff >empty.out &&
	test_must_be_empty empty.out
'

test_expect_success 'patch-id with multiple hunks in one file' '
	cat >multi.diff <<-\EOF &&
	commit eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
	diff --git a/file b/file
	index 1234567..abcdefg 100644
	--- a/file
	+++ b/file
	@@ -1,3 +1,4 @@
	 line1
	+added1
	 line2
	 line3
	@@ -10,3 +11,4 @@
	 line10
	+added2
	 line11
	 line12
	EOF
	git patch-id <multi.diff >multi.out &&
	grep "^$OID_REGEX $OID_REGEX$" multi.out
'

test_expect_success 'patch-id stable: same diff same id regardless of run' '
	cd repo &&
	git show HEAD >run1.out &&
	git patch-id --stable <run1.out >pid_run1 &&
	git show HEAD >run2.out &&
	git patch-id --stable <run2.out >pid_run2 &&
	sed "s/ .*//" pid_run1 >id1 &&
	sed "s/ .*//" pid_run2 >id2 &&
	test_cmp id1 id2
'

test_expect_success 'patch-id with rename diff produces output' '
	cat >rename.diff <<-\EOF &&
	commit ffffffffffffffffffffffffffffffffffffffff
	diff --git a/old b/new
	similarity index 100%
	rename from old
	rename to new
	EOF
	git patch-id <rename.diff >rename.out &&
	grep "^$OID_REGEX $OID_REGEX$" rename.out
'

test_expect_success 'patch-id default is same as --stable' '
	cd repo &&
	git show HEAD >def.out &&
	git patch-id <def.out >pid_def &&
	git patch-id --stable <def.out >pid_stable &&
	test_cmp pid_def pid_stable
'

test_expect_success 'patch-id detects equality for equivalent patches' '
	cd repo &&
	git show HEAD >p1 &&
	git show HEAD >p2 &&
	git patch-id <p1 >id1 &&
	git patch-id <p2 >id2 &&
	sed "s/ .*//" id1 >id1_hash &&
	sed "s/ .*//" id2 >id2_hash &&
	test_cmp id1_hash id2_hash
'

test_expect_success 'patch-id detects inequality for different patches' '
	cd repo &&
	git show HEAD >p1 &&
	git show HEAD~1 >p2 &&
	git patch-id <p1 >id1 &&
	git patch-id <p2 >id2 &&
	sed "s/ .*//" id1 >id1_hash &&
	sed "s/ .*//" id2 >id2_hash &&
	! test_cmp id1_hash id2_hash
'

test_expect_success 'patch-id handles no-nl-at-eof markers' '
	cd repo &&
	cat >nonl <<-\EOF &&
	commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
	diff --git a/x b/x
	index e69de29..2e65efe 100644
	--- a/x
	+++ b/x
	@@ -0,0 +1 @@
	+a
	\ No newline at end of file
	EOF
	git patch-id <nonl >nonl.out &&
	grep "^$OID_REGEX $OID_REGEX$" nonl.out
'

test_expect_success 'patch-id handles diffs with one line of before/after' '
	cd repo &&
	cat >diffu1 <<-\EOF &&
	commit bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
	diff --git a/bar b/bar
	index bdaf90f..31051f6 100644
	--- a/bar
	+++ b/bar
	@@ -2 +2,2 @@
	 b
	+c
	EOF
	git patch-id <diffu1 >diffu1.out &&
	grep "^$OID_REGEX $OID_REGEX$" diffu1.out
'

test_expect_success 'patch-id on empty diff produces no output' '
	cd repo &&
	git patch-id </dev/null >empty.out &&
	test_must_be_empty empty.out
'

test_expect_success 'patch-id with binary diff still produces output' '
	cat >binary.diff <<-\EOF &&
	commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
	diff --git a/bin b/bin
	new file mode 100644
	index 0000000..1234567
	Binary files /dev/null and b/bin differ
	EOF
	git patch-id <binary.diff >binary.out &&
	grep "^$OID_REGEX $OID_REGEX$" binary.out
'

test_done
