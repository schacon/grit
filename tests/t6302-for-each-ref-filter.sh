#!/bin/sh
# Ported subset from git/t/t6302-for-each-ref-filter.sh.

test_description='for-each-ref filtering options'

. ./test-lib.sh

setup_graph () {
	git init repo &&
	cd repo &&

	EMPTY_TREE=$(printf "" | git hash-object -w -t tree --stdin) &&
	A=$(git commit-tree "$EMPTY_TREE" -m one) &&
	B=$(git commit-tree "$EMPTY_TREE" -p "$A" -m two) &&
	C=$(git commit-tree "$EMPTY_TREE" -p "$B" -m three) &&
	D=$(git commit-tree "$EMPTY_TREE" -p "$B" -m four) &&

	git update-ref refs/heads/main "$C" &&
	git update-ref refs/heads/side "$D" &&
	git update-ref refs/odd/spot "$C" &&
	git update-ref refs/tags/one "$A" &&
	git update-ref refs/tags/two "$B" &&
	git update-ref refs/tags/three "$C" &&
	git update-ref refs/tags/four "$D" &&
	git symbolic-ref HEAD refs/heads/main
}

test_expect_success 'setup history and refs' '
	setup_graph
'

test_expect_success 'filtering with --points-at' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --points-at=main >actual &&
	test_cmp expect actual
'

test_expect_success 'filtering with --merged' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --merged=main >actual &&
	test_cmp expect actual
'

test_expect_success 'filtering with --no-merged' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --no-merged=main >actual &&
	test_cmp expect actual
'

test_expect_success 'filtering with --contains' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/four
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains=two >actual &&
	test_cmp expect actual
'

test_expect_success 'filtering with --no-contains' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --no-contains=two >actual &&
	test_cmp expect actual
'

test_expect_success 'filtering with --contains and --no-contains' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains=two --no-contains=three >actual &&
	test_cmp expect actual
'

test_done
