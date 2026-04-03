#!/bin/sh
#
# Ported subset from git/t/t4103-apply-binary.sh

test_description='git apply handling binary patches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	cat >file1 <<-\EOF &&
	A quick brown fox jumps over the lazy dog.
	A tiny little penguin runs around in circles.
	There is a flag with Linux written on it.
	A slow black-and-white panda just sits there,
	munching on his bamboo.
	EOF
	git add file1 &&
	git commit -m "Initial Version" &&
	git tag initial
'

test_expect_success 'stat binary diff -- should not fail' '
	cd repo &&
	cat >B.diff <<-\EOF &&
	diff --git a/file1 b/file1
	index 1234567..abcdefg 100644
	Binary files a/file1 and b/file1 differ
	EOF
	git apply --stat --summary B.diff
'

test_expect_success 'stat on patch with new file' '
	cd repo &&
	cat >new.diff <<-\EOF &&
	diff --git a/newbin b/newbin
	new file mode 100644
	index 0000000..1234567
	Binary files /dev/null and b/newbin differ
	EOF
	git apply --stat new.diff >output &&
	test_grep "newbin" output
'

test_expect_success 'numstat binary diff' '
	cd repo &&
	git apply --numstat B.diff >output &&
	test_grep "file1" output
'

test_done
