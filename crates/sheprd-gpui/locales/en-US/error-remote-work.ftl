# Why new work on a computer could not start: a new worktree there, and an
# agent started in it with its first prompt. $error is the failure
# underneath, in Git's, the shell's, or herdr-core's own words.

# $step is the part of the work that failed.
error-remote-work-script = { $step ->
    [create-worktree] creating the worktree failed: { $error }
    [list] listing the repository's worktrees failed: { $error }
    [prompt] sending the agent its prompt failed: { $error }
    [remove] removing the worktree failed: { $error }
   *[start-agent] starting the agent failed: { $error }
}
# $step is the part of the work whose script printed something unexpected.
error-remote-work-output = { $step ->
    [create-worktree] creating the worktree failed: unexpected output
    [list] listing the repository's worktrees failed: unexpected output
    [prompt] sending the agent its prompt failed: unexpected output
    [remove] removing the worktree failed: unexpected output
   *[start-agent] starting the agent failed: unexpected output
}
error-remote-work-agent = The agent did not take its prompt: { $error }
error-remote-work-cancelled = The work was cancelled
error-remote-work-dirty = The worktree has uncommitted changes
error-remote-work-unprompted = The agent started but did not take its prompt: { $error }
