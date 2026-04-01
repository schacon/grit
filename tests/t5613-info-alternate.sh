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

test_done
