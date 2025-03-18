## Development Approach

bae follows _README-driven development_.

### Process

1. Features are documented in this README
2. Implementation tasks are broken down in TASKS.md with specific, actionable
   steps (either by hand or using LLM assistance)
3. Code is written by LLMs based on these descriptions and tasks
4. Results are reviewed and tested by humans
5. Documentation is updated based on implementation learnings
6. If implementation fails, the documentation and task breakdown are improved
   until they're clear enough for LLM implementation

Both feature descriptions and task breakdowns must be good enough for an LLM to
implement.

### Motivation

This approach was selected to:

- Preserve valuable prompts and LLM interactions as part of the codebase
- Retain design context that would otherwise be lost after coding sessions
- Maintain technical documentation that evolves alongside the implementation
- Create self-documenting code and capture thought process
- Facilitate collaboration between contributors across time
