#!/bin/sh
# Adapted from git/t/t0030-stripspace.sh
# Tests for 'grit stripspace'.

test_description='git stripspace'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

t40='A quick brown fox jumps over the lazy do'
s40='                                        '
sss="$s40$s40$s40$s40$s40$s40$s40$s40$s40$s40" # 400 spaces
ttt="$t40$t40$t40$t40$t40$t40$t40$t40$t40$t40" # 400 chars of text

# Run git stripspace on printf-formatted input and capture stdout.
printf_git_stripspace () {
    printf "$1" | git stripspace
}

test_expect_success 'long lines without spaces should be unchanged' '
    echo "$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual &&

    echo "$ttt$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual &&

    echo "$ttt$ttt$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual &&

    echo "$ttt$ttt$ttt$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual
'

test_expect_success 'lines with spaces at the beginning should be unchanged' '
    echo "$sss$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual &&

    echo "$sss$sss$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual &&

    echo "$sss$sss$sss$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual
'

test_expect_success 'lines with intermediate spaces should be unchanged' '
    echo "$ttt$sss$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual &&

    echo "$ttt$sss$sss$ttt" >expect &&
    git stripspace <expect >actual &&
    test_cmp expect actual
'

test_expect_success 'consecutive blank lines should be unified' '
    printf "$ttt\n\n$ttt\n" > expect &&
    printf "$ttt\n\n\n\n\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt$ttt\n\n$ttt\n" > expect &&
    printf "$ttt$ttt\n\n\n\n\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n\n$ttt\n" > expect &&
    printf "$ttt\n\t\n \n\n  \t\t\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt$ttt\n\n$ttt\n" > expect &&
    printf "$ttt$ttt\n\t\n \n\n  \t\t\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual
'

test_expect_success 'only consecutive blank lines should be completely removed' '
    printf "\n" | git stripspace >actual &&
    test_must_be_empty actual &&

    printf "\n\n\n" | git stripspace >actual &&
    test_must_be_empty actual &&

    printf "$sss\n$sss\n$sss\n" | git stripspace >actual &&
    test_must_be_empty actual &&

    printf "$sss$sss\n$sss\n\n" | git stripspace >actual &&
    test_must_be_empty actual &&

    printf "\n$sss\n$sss$sss\n" | git stripspace >actual &&
    test_must_be_empty actual
'

test_expect_success 'consecutive blank lines at the beginning should be removed' '
    printf "$ttt\n" > expect &&
    printf "\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n" > expect &&
    printf "\n\n\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt$ttt\n" > expect &&
    printf "\n\n\n$ttt$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n" > expect &&

    printf "$sss\n$sss\n$sss\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "\n$sss\n$sss$sss\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$sss$sss\n$sss\n\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual
'

test_expect_success 'consecutive blank lines at the end should be removed' '
    printf "$ttt\n" > expect &&
    printf "$ttt\n\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n" > expect &&
    printf "$ttt\n\n\n\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt$ttt\n" > expect &&
    printf "$ttt$ttt\n\n\n\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n" > expect &&

    printf "$ttt\n$sss\n$sss\n$sss\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n\n$sss\n$sss$sss\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n$sss$sss\n$sss\n\n" | git stripspace >actual &&
    test_cmp expect actual
'

test_expect_success 'text without newline at end should end with newline' '
    printf_git_stripspace "$ttt" >out &&
    test_line_count -gt 0 out &&

    printf_git_stripspace "$ttt$ttt" >out &&
    test_line_count -gt 0 out &&

    printf_git_stripspace "$ttt$ttt$ttt" >out &&
    test_line_count -gt 0 out
'

test_expect_success 'text plus spaces without newline should show the correct lines' '
    printf "$ttt\n" >expect &&
    printf "$ttt$sss" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n" >expect &&
    printf "$ttt$sss$sss" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt$ttt\n" >expect &&
    printf "$ttt$ttt$sss" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt$ttt$ttt\n" >expect &&
    printf "$ttt$ttt$ttt$sss" | git stripspace >actual &&
    test_cmp expect actual
'

test_expect_success 'text plus spaces at end should be cleaned and newline must remain' '
    echo "$ttt" >expect &&
    echo "$ttt$sss" | git stripspace >actual &&
    test_cmp expect actual &&

    echo "$ttt" >expect &&
    echo "$ttt$sss$sss" | git stripspace >actual &&
    test_cmp expect actual &&

    echo "$ttt$ttt" >expect &&
    echo "$ttt$ttt$sss" | git stripspace >actual &&
    test_cmp expect actual &&

    echo "$ttt$ttt$ttt" >expect &&
    echo "$ttt$ttt$ttt$sss" | git stripspace >actual &&
    test_cmp expect actual
'

test_expect_success 'spaces with newline at end should be replaced with empty string' '
    echo | git stripspace >actual &&
    test_must_be_empty actual &&

    echo "$sss" | git stripspace >actual &&
    test_must_be_empty actual &&

    echo "$sss$sss" | git stripspace >actual &&
    test_must_be_empty actual &&

    echo "$sss$sss$sss" | git stripspace >actual &&
    test_must_be_empty actual
'

test_expect_success 'spaces without newline at end should be replaced with empty string' '
    printf "" | git stripspace >actual &&
    test_must_be_empty actual &&

    printf "$sss$sss" | git stripspace >actual &&
    test_must_be_empty actual &&

    printf "$sss$sss$sss" | git stripspace >actual &&
    test_must_be_empty actual
'

test_expect_success 'text plus spaces without newline at end should not show spaces' '
    printf "$ttt$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    printf "$ttt$ttt$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    printf "$ttt$ttt$ttt$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    printf "$ttt$sss$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    printf "$ttt$ttt$sss$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    printf "$ttt$sss$sss$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null
'

test_expect_success 'text plus spaces at end should not show spaces' '
    echo "$ttt$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    echo "$ttt$ttt$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    echo "$ttt$ttt$ttt$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    echo "$ttt$sss$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    echo "$ttt$ttt$sss$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null &&
    echo "$ttt$sss$sss$sss" | git stripspace >tmp &&
    ! grep "  " tmp >/dev/null
'

test_expect_success 'spaces without newline at end should not show spaces' '
    printf "" | git stripspace >tmp &&
    ! grep " " tmp >/dev/null &&
    printf "$sss" | git stripspace >tmp &&
    ! grep " " tmp >/dev/null &&
    printf "$sss$sss" | git stripspace >tmp &&
    ! grep " " tmp >/dev/null &&
    printf "$sss$sss$sss" | git stripspace >tmp &&
    ! grep " " tmp >/dev/null &&
    printf "$sss$sss$sss$sss" | git stripspace >tmp &&
    ! grep " " tmp >/dev/null
'

test_expect_success 'consecutive text lines should be unchanged' '
    printf "$ttt$ttt\n$ttt\n" >expect &&
    printf "$ttt$ttt\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n$ttt$ttt\n$ttt\n" >expect &&
    printf "$ttt\n$ttt$ttt\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual &&

    printf "$ttt\n$ttt\n\n$ttt$ttt\n$ttt\n" >expect &&
    printf "$ttt\n$ttt\n\n$ttt$ttt\n$ttt\n" | git stripspace >actual &&
    test_cmp expect actual
'

test_expect_success 'strip comments, too' '
    test ! -z "$(echo "# comment" | git stripspace)" &&
    test -z "$(echo "# comment" | git stripspace -s)"
'

test_expect_success '-c with single line' '
    printf "# foo\n" >expect &&
    printf "foo" | git stripspace -c >actual &&
    test_cmp expect actual
'

test_expect_success '-c with single line followed by empty line' '
    printf "# foo\n#\n" >expect &&
    printf "foo\n\n" | git stripspace -c >actual &&
    test_cmp expect actual
'

test_expect_success '-c with newline only' '
    printf "#\n" >expect &&
    printf "\n" | git stripspace -c >actual &&
    test_cmp expect actual
'

test_expect_success '--comment-lines with single line' '
    printf "# foo\n" >expect &&
    printf "foo" | git stripspace --comment-lines >actual &&
    test_cmp expect actual
'

test_expect_success 'avoid SP-HT sequence in commented line' '
    printf "#\tone\n#\n# two\n" >expect &&
    printf "\tone\n\ntwo\n" | git stripspace -c >actual &&
    test_cmp expect actual
'

test_done
