lets start or continue with the next phase of @IMPL.md. mark completed when done, then write a short and concise commit message for staged and unstaged changes, and untracked files and commit and push. after each phase, wait for further instructions.

use subagents whenever possible to speed things up and save context.
when spawning subagents, select the appropriate model:
- haiku: quick tasks like searching, grepping, running tests
- sonnet: moderate tasks like code exploration, multi-step implementation, reviewing
- opus: complex reasoning, planning
spawn subagents in parallel, but make sure they can coordinate when touching same code.

only if there is nothing left to do (i.e., all todos have been marked as completed), signal the loop to stop by running: `touch loop/BREAK`, but make sure there are no empty checkboxes.
