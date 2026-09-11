# Agent Instructions

## Screenshots and privacy

Never capture screenshots of the user's system, desktop, windows, or applications,
including through tools, scripts, or subagents. When visual evidence is needed,
ask the user to take and provide a screenshot instead.

## Commits and pull requests

Use [Conventional Commits](https://www.conventionalcommits.org/) for every commit message and pull-request title:

```text
<type>[optional scope]: <description>
```

Use the type that best describes the change, such as `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, or `chore`.

Write descriptions in lowercase imperative form without a trailing period. Mark breaking changes with `!` or a `BREAKING CHANGE:` footer as defined by the convention.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Triage uses the five default canonical labels. See `docs/agents/triage-labels.md`.

### Domain docs

Domain documentation uses a single-context layout. See `docs/agents/domain.md`.
