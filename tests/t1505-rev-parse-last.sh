#!/bin/sh
# Test rev-parse: branch resolution, tag peeling, --verify, --short,
# --git-dir, --show-toplevel, --show-prefix, --is-inside-work-tree,
# --is-bare-repository, ^{commit}, ^{tree}, HEAD:path, and more.

test_description='grit rev-parse extended features'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################################
# Section 1: Setup
###########################################################################

test_expect_success 'setup repository with branches and tags' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "file-a" >a.txt &&
	grit add a.txt &&
	grit commit -m "first commit" &&
	grit branch feature &&
	grit checkout feature &&
	echo "file-b" >b.txt &&
	grit add b.txt &&
	grit commit -m "second commit" &&
	grit checkout master &&
	grit tag v1.0 HEAD &&
	grit tag -a -m "annotated tag" v2.0 HEAD
'

###########################################################################
# Section 2: Basic ref resolution
###########################################################################

test_expect_success 'rev-parse HEAD produces 40-char hex' '
	cd repo &&
	grit rev-parse HEAD >actual &&
	test "$(wc -c <actual | tr -d " ")" -ge 40
'

test_expect_success 'rev-parse master resolves to commit' '
	cd repo &&
	grit rev-parse master >actual &&
	OID=$(cat actual) &&
	grit cat-file -t "$OID" >type &&
	echo "commit" >expect &&
	test_cmp expect type
'

test_expect_success 'rev-parse feature resolves to its tip' '
	cd repo &&
	grit rev-parse feature >actual &&
	OID=$(cat actual) &&
	grit cat-file -t "$OID" >type &&
	echo "commit" >expect &&
	test_cmp expect type
'

test_expect_success 'rev-parse refs/heads/master equals master' '
	cd repo &&
	grit rev-parse master >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse with multiple args' '
	cd repo &&
	grit rev-parse master feature >actual &&
	test_line_count = 2 actual
'

test_expect_success 'rev-parse HEAD HEAD produces same OID twice' '
	cd repo &&
	grit rev-parse HEAD HEAD >actual &&
	test_line_count = 2 actual &&
	head -1 actual >first &&
	tail -1 actual >second &&
	test_cmp first second
'

###########################################################################
# Section 3: Tag resolution and peeling
###########################################################################

test_expect_success 'rev-parse lightweight tag equals HEAD' '
	cd repo &&
	grit rev-parse v1.0 >actual &&
	grit rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse annotated tag yields tag object' '
	cd repo &&
	OID=$(grit rev-parse v2.0) &&
	grit cat-file -t "$OID" >actual &&
	echo "tag" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse v2.0^{commit} peels to commit' '
	cd repo &&
	OID=$(grit rev-parse v2.0^{commit}) &&
	grit cat-file -t "$OID" >actual &&
	echo "commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'peeled annotated tag equals HEAD' '
	cd repo &&
	grit rev-parse v2.0^{commit} >actual &&
	grit rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse v2.0^{} peels annotated tag' '
	cd repo &&
	grit rev-parse v2.0^{} >actual &&
	grit rev-parse HEAD >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 4: ^{type} peel syntax
###########################################################################

test_expect_success 'HEAD^{commit} resolves same as HEAD' '
	cd repo &&
	grit rev-parse HEAD^{commit} >actual &&
	grit rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_expect_success 'HEAD^{tree} resolves to tree object' '
	cd repo &&
	OID=$(grit rev-parse HEAD^{tree}) &&
	grit cat-file -t "$OID" >actual &&
	echo "tree" >expect &&
	test_cmp expect actual
'

test_expect_success 'HEAD^0 resolves same as HEAD^{commit}' '
	cd repo &&
	grit rev-parse HEAD^0 >actual &&
	grit rev-parse HEAD^{commit} >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 5: HEAD:path syntax
###########################################################################

test_expect_success 'rev-parse HEAD:a.txt resolves to blob' '
	cd repo &&
	OID=$(grit rev-parse HEAD:a.txt) &&
	grit cat-file -t "$OID" >actual &&
	echo "blob" >expect &&
	test_cmp expect actual
'

test_expect_success 'HEAD:a.txt blob content matches file' '
	cd repo &&
	OID=$(grit rev-parse HEAD:a.txt) &&
	grit cat-file -p "$OID" >actual &&
	echo "file-a" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse feature:b.txt resolves to blob on feature branch' '
	cd repo &&
	OID=$(grit rev-parse feature:b.txt) &&
	grit cat-file -t "$OID" >actual &&
	echo "blob" >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 6: --verify
###########################################################################

test_expect_success 'rev-parse --verify HEAD succeeds' '
	cd repo &&
	grit rev-parse --verify HEAD >actual &&
	test -s actual
'

test_expect_success 'rev-parse --verify with valid branch' '
	cd repo &&
	grit rev-parse --verify master >actual &&
	test -s actual
'

test_expect_success 'rev-parse --verify fails on bogus ref' '
	cd repo &&
	test_must_fail grit rev-parse --verify bogus-ref 2>err &&
	grep -i -e "single revision" -e "needed" err
'

test_expect_success 'rev-parse --verify --quiet suppresses error' '
	cd repo &&
	test_must_fail grit rev-parse --verify --quiet nonexistent 2>err &&
	test_must_be_empty err
'

###########################################################################
# Section 7: --short
###########################################################################

test_expect_success 'rev-parse --short produces abbreviated hash' '
	cd repo &&
	grit rev-parse --short HEAD >actual &&
	LEN=$(wc -c <actual | tr -d " ") &&
	test "$LEN" -lt 41
'

test_expect_success 'abbreviated hash is prefix of full hash' '
	cd repo &&
	FULL=$(grit rev-parse HEAD) &&
	SHORT=$(grit rev-parse --short HEAD) &&
	case "$FULL" in
	${SHORT}*) true ;;
	*) false ;;
	esac
'

###########################################################################
# Section 8: Repository info flags
###########################################################################

test_expect_success 'rev-parse --git-dir returns .git' '
	cd repo &&
	grit rev-parse --git-dir >actual &&
	echo ".git" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --show-toplevel returns repo root' '
	cd repo &&
	TOPLEVEL=$(grit rev-parse --show-toplevel) &&
	test -d "$TOPLEVEL/.git"
'

test_expect_success 'rev-parse --is-inside-work-tree returns true' '
	cd repo &&
	grit rev-parse --is-inside-work-tree >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --is-bare-repository returns false' '
	cd repo &&
	grit rev-parse --is-bare-repository >actual &&
	echo "false" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --show-prefix in subdirectory' '
	cd repo &&
	mkdir -p sub/dir &&
	(cd sub/dir && grit rev-parse --show-prefix >../../prefix_actual) &&
	echo "sub/dir/" >prefix_expect &&
	test_cmp prefix_expect prefix_actual
'

test_expect_success 'rev-parse --show-prefix at repo root is empty or newline' '
	cd repo &&
	grit rev-parse --show-prefix >actual &&
	test "$(cat actual)" = ""
'

###########################################################################
# Section 9: Error cases
###########################################################################

test_expect_success 'rev-parse fails on invalid ref' '
	cd repo &&
	test_must_fail grit rev-parse not-a-ref 2>err
'

test_expect_success 'rev-parse fails on nonexistent ref name' '
	cd repo &&
	test_must_fail grit rev-parse --verify does-not-exist-at-all 2>err
'

test_expect_success 'rev-parse --verify with two args fails or warns' '
	cd repo &&
	test_must_fail grit rev-parse --verify HEAD master 2>err
'

###########################################################################
# Section 10: Bare repository
###########################################################################

test_expect_success 'rev-parse --is-bare-repository in bare repo' '
	grit init --bare bare-repo.git &&
	grit -C bare-repo.git rev-parse --is-bare-repository >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --git-dir in bare repo' '
	grit -C bare-repo.git rev-parse --git-dir >actual &&
	echo "." >expect &&
	test_cmp expect actual
'

test_done
