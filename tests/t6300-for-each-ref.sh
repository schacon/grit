#!/bin/sh
# Ported subset from git/t/t6300-for-each-ref.sh.

test_description='for-each-ref basic behaviors'

. ./test-lib.sh

setup_graph () {
	git init repo &&
	cd repo &&

	EMPTY_TREE=$(printf "" | git hash-object -w -t tree --stdin) &&

	cat >msg-A <<-\EOF &&
A
EOF
	A=$(git commit-tree "$EMPTY_TREE" -m A) &&

	cat >msg-B <<-\EOF &&
B
EOF
	B=$(git commit-tree "$EMPTY_TREE" -p "$A" -m B) &&

	cat >msg-C <<-\EOF &&
C
EOF
	C=$(git commit-tree "$EMPTY_TREE" -p "$B" -m C) &&

	cat >msg-D <<-\EOF &&
D
EOF
	D=$(git commit-tree "$EMPTY_TREE" -p "$B" -m D) &&

	git update-ref refs/heads/main "$C" &&
	git update-ref refs/heads/side "$D" &&
	git update-ref refs/odd/spot "$C" &&
	git update-ref refs/tags/one "$A" &&
	git update-ref refs/tags/two "$B" &&
	git update-ref refs/tags/three "$C" &&
	git update-ref refs/tags/four "$D"
}

test_expect_success 'setup history and refs' '
	setup_graph
'

test_expect_success 'for-each-ref help text is available' '
	cd repo &&
	git for-each-ref --help >usage 2>&1 &&
	test -s usage
'

test_expect_success 'default ordering by refname' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/four
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" >actual &&
	test_cmp expect actual
'

test_expect_success 'descending sort and count' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --sort=-refname --count=2 >actual &&
	test_cmp expect actual
'

test_expect_success 'prefix patterns and --exclude' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/one
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" refs/tags --exclude=refs/tags/three >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin patterns work' '
	cd repo &&
	cat >patterns <<-\EOF &&
refs/heads/*
refs/tags/t*
EOF
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --stdin <patterns >actual &&
	test_cmp expect actual
'

test_done
