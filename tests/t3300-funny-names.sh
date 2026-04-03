#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#

test_description='Pathnames with funny characters.

This test tries pathnames with funny characters in the working
tree, index, and tree objects.
'

. ./test-lib.sh

HT='	'

# Check if filesystem supports tabs
echo 2>/dev/null > "Name with an${HT}HT"
if ! test -f "Name with an${HT}HT"
then
	echo "1..0 # SKIP filesystem does not allow tabs in filenames"
	test_done
fi
rm -f "Name with an${HT}HT"

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

p0='no-funny'
p1='tabs	," (dq) and spaces'
p2='just space'

test_expect_success 'setup' '
	cat >"$p0" <<-\EOF &&
	1. A quick brown fox jumps over the lazy cat, oops dog.
	2. A quick brown fox jumps over the lazy cat, oops dog.
	3. A quick brown fox jumps over the lazy cat, oops dog.
	EOF

	{ cat "$p0" >"$p1" || :; } &&
	{ echo "Foo Bar Baz" >"$p2" || :; }
'

test_expect_success 'setup: populate index and tree' '
	git update-index --add "$p0" "$p2" &&
	t0=$(git write-tree)
'

test_expect_success 'ls-files prints space in filename verbatim' '
	printf "%s\n" "just space" no-funny >expected &&
	git ls-files >current &&
	test_cmp expected current
'

test_expect_success 'setup: add funny filename' '
	git update-index --add "$p1" &&
	t1=$(git write-tree)
'

test_expect_success 'ls-files -z does not quote funny filename' '
	cat >expected <<-\EOF &&
	just space
	no-funny
	tabs	," (dq) and spaces
	EOF
	git ls-files -z >ls-files.z &&
	tr "\000" "\012" <ls-files.z >current &&
	test_cmp expected current
'

test_done
