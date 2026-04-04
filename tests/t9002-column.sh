#!/bin/sh
# Ported from git/t/t9002-column.sh
# Tests for 'git column' command

test_description='git column'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	cat >lista <<\EOF
one
two
three
four
five
six
seven
eight
nine
ten
eleven
EOF
'

test_expect_success 'never' '
	git column --mode=never <lista >actual &&
	test_cmp lista actual
'

test_expect_success 'always (plain mode)' '
	cat >expected <<\EOF &&
Zone
Ztwo
Zthree
Zfour
Zfive
Zsix
Zseven
Zeight
Znine
Zten
Zeleven
EOF
	git column --indent=Z --mode=plain <lista >actual &&
	test_cmp expected actual
'

test_expect_success '80 columns' '
	cat >expected <<\EOF &&
one    two    three  four   five   six    seven  eight  nine   ten    eleven
EOF
	git column --mode=column --width=80 <lista >actual &&
	test_cmp expected actual
'

test_expect_success 'width = 1' '
	git column --mode=column --width=1 <lista >actual &&
	test_cmp lista actual
'

test_expect_success '20 columns' '
	cat >expected <<\EOF &&
one    seven
two    eight
three  nine
four   ten
five   eleven
six
EOF
	git column --mode=column --width=20 <lista >actual &&
	test_cmp expected actual
'

test_expect_success '20 columns, padding 2' '
	cat >expected <<\EOF &&
one     seven
two     eight
three   nine
four    ten
five    eleven
six
EOF
	git column --mode=column --width=20 --padding 2 <lista >actual &&
	test_cmp expected actual
'

test_expect_success '20 columns, row first' '
	cat >expected <<\EOF &&
one    two
three  four
five   six
seven  eight
nine   ten
eleven
EOF
	git column --mode=row --width=20 <lista | sed "s/ *$//" >actual &&
	test_cmp expected actual
'

test_expect_failure '20 columns, dense' '
	cat >expected <<\EOF &&
one   five  nine
two   six   ten
three seven eleven
four  eight
EOF
	git column --mode=column,dense --width=20 <lista >actual &&
	test_cmp expected actual
'

test_expect_failure '20 columns, nodense' '
	cat >expected <<\EOF &&
one    seven
two    eight
three  nine
four   ten
five   eleven
six
EOF
	git column --mode=column,nodense --width=20 <lista >actual &&
	test_cmp expected actual
'

test_expect_failure '20 columns, row first, dense' '
	cat >expected <<\EOF &&
one   two    three
four  five   six
seven eight  nine
ten   eleven
EOF
	git column --mode=row,dense --width=20 <lista >actual &&
	test_cmp expected actual
'

test_expect_failure '--nl option' '
	cat >expected <<\EOF &&
oneZ
twoZ
threeZ
fourZ
fiveZ
sixZ
sevenZ
eightZ
nineZ
tenZ
elevenZ
EOF
	git column --nl="Z$LF" --mode=plain <lista >actual &&
	test_cmp expected actual
'

test_expect_failure 'negative padding rejected' '
	test_must_fail git column --mode=column --padding=-1 <lista 2>err &&
	grep "non-negative" err
'

test_done
