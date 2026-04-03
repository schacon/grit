#!/bin/sh

test_description='git blame with specific diff algorithm'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init blame-diff-algo &&
	cd blame-diff-algo &&
	cat >file.c <<-\EOF &&
	int f(int x, int y)
	{
	  if (x == 0)
	  {
	    return y;
	  }
	  return x;
	}

	int g(size_t u)
	{
	  while (u < 30)
	  {
	    u++;
	  }
	  return u;
	}
	EOF
	test_write_lines x x x x >file.txt &&
	git add file.c file.txt &&
	GIT_AUTHOR_NAME=Commit_1 git commit -m Commit_1 &&

	cat >file.c <<-\EOF &&
	int g(size_t u)
	{
	  while (u < 30)
	  {
	    u++;
	  }
	  return u;
	}

	int h(int x, int y, int z)
	{
	  if (z == 0)
	  {
	    return x;
	  }
	  return y;
	}
	EOF
	test_write_lines x x x A B C D x E F G >file.txt &&
	git add file.c file.txt &&
	GIT_AUTHOR_NAME=Commit_2 git commit -m Commit_2
'

test_expect_success 'blame uses Myers diff algorithm by default' '
	cd blame-diff-algo &&
	cat >expected <<-\EOF &&
	Commit_2 int g(size_t u)
	Commit_1 {
	Commit_2   while (u < 30)
	Commit_1   {
	Commit_2     u++;
	Commit_1   }
	Commit_2   return u;
	Commit_1 }
	Commit_1
	Commit_2 int h(int x, int y, int z)
	Commit_1 {
	Commit_2   if (z == 0)
	Commit_1   {
	Commit_2     return x;
	Commit_1   }
	Commit_2   return y;
	Commit_1 }
	EOF

	git blame file.c >output &&
	sed -e "s/^[^ ]* (\([^ ]*\) [^)]*)/\1/g" output >without_varying_parts &&
	sed -e "s/ *$//g" without_varying_parts >actual &&
	test_cmp expected actual
'

test_expect_success 'blame with --line-porcelain output' '
	cd blame-diff-algo &&
	git blame --line-porcelain file.c >output &&
	grep "^author " output >authors &&
	test $(wc -l <authors) -eq 17
'

test_done
