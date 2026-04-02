#!/bin/sh
# Ported subset from git/t/t5613-info-alternate.sh.

test_description='count-objects -v reports transitive alternates'

. ./test-lib.sh

test_expect_success 'setup alternate chain A <- B <- C' '
	git init --bare A &&
	git init --bare B &&
	git init --bare C &&
	echo "$(pwd)/A/objects" >B/objects/info/alternates &&
	echo "$(pwd)/B/objects" >C/objects/info/alternates
'

test_expect_success 'count-objects shows transitive alternates' '
	cat >expect <<-EOF &&
	alternate: $(pwd)/B/objects
	alternate: $(pwd)/A/objects
	EOF
	git -C C count-objects -v >actual &&
	grep "^alternate:" actual >actual.alternates &&
	test_cmp expect actual.alternates
'

test_expect_success 'loop in alternates does not hang' '
	echo "$(pwd)/B/objects" >>A/objects/info/alternates &&
	cat >expect <<-EOF &&
	alternate: $(pwd)/B/objects
	alternate: $(pwd)/A/objects
	EOF
	git -C C count-objects -v >actual &&
	grep "^alternate:" actual >actual.alternates &&
	test_cmp expect actual.alternates
'

test_expect_success 'deep alternate chain is traversed fully' '
	git init --bare D &&
	echo "$(pwd)/C/objects" >D/objects/info/alternates &&
	git init --bare E &&
	echo "$(pwd)/D/objects" >E/objects/info/alternates &&
	git init --bare F &&
	echo "$(pwd)/E/objects" >F/objects/info/alternates &&
	cat >expect <<-EOF &&
	alternate: $(pwd)/E/objects
	alternate: $(pwd)/D/objects
	alternate: $(pwd)/C/objects
	alternate: $(pwd)/B/objects
	alternate: $(pwd)/A/objects
	EOF
	git -C F count-objects -v >actual &&
	grep "^alternate:" actual >actual.alternates &&
	test_cmp expect actual.alternates
'

test_expect_success 'relative duplicate alternates are eliminated' '
	mkdir -p deep/subdir &&
	git init --bare deep/subdir/duplicate.git &&
	cat >deep/subdir/duplicate.git/objects/info/alternates <<-\EOF &&
	../../../../C/objects
	../../../../A/objects
	EOF
	cat >expect <<-EOF &&
	alternate: $(pwd)/C/objects
	alternate: $(pwd)/B/objects
	alternate: $(pwd)/A/objects
	EOF
	git -C deep/subdir/duplicate.git count-objects -v >actual &&
	grep "^alternate:" actual >actual.alternates &&
	test_cmp expect actual.alternates
'

test_expect_success 'self-referencing alternate does not hang count-objects' '
	echo "$(pwd)/C/objects" >>C/objects/info/alternates &&
	git -C C count-objects -v >actual &&
	grep "^alternate:" actual >actual.alternates &&
	# Should not hang even with self-reference
	test -s actual.alternates
'

test_expect_success 'alternate chain with four levels' '
	git init --bare D &&
	echo "$(pwd)/C/objects" >D/objects/info/alternates &&
	cat >expect <<-EOF &&
	alternate: $(pwd)/C/objects
	alternate: $(pwd)/B/objects
	alternate: $(pwd)/A/objects
	EOF
	git -C D count-objects -v >actual &&
	grep "^alternate:" actual >actual.alternates &&
	test_cmp expect actual.alternates
'

test_expect_success 'empty alternates file is handled gracefully' '
	git init --bare empty-alt &&
	: >empty-alt/objects/info/alternates &&
	git -C empty-alt count-objects -v >actual &&
	! grep "^alternate:" actual
'

test_expect_success 'nonexistent alternate path does not crash count-objects' '
	git init --bare bad-alt &&
	echo "/nonexistent/path/objects" >bad-alt/objects/info/alternates &&
	git -C bad-alt count-objects -v >actual 2>&1
'

test_done
