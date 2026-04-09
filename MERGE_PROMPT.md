Check for open PRs on GitHub.
Choose one, fetch the associated branch down, try to merge it locally.
If there are merge conflicts, then try to resolve them.
For any conflicts in the docs/ or data/ directories, simply resolve with an ours strategy.
For other conflicts, look at the PR description, commit messages, and diff of the PR branch and try to intelligently replay the intended changes on the current state of the code.
Continue until all conflicts are resolved.
Commit with the PR description in the merge commit message.
Push back up to origin/main.
