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

test_done
