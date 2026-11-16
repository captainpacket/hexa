---
name: hexa-summarize
description: Give an agent a file's context without reading the whole tree — what it defines, what it uses, who consumes it. Use when the user asks to "summarize project", "ast summary", "token context", "generate context", "project overview", or "what does this project do".
---

# hexa summarize — context from the code graph

hexa has no summarize verb, and hexa is not an npm package, so never put npx
in front of it. The context this skill used to promise comes from the code graph,
which hexa builds from the source and saves in `graph-out/graph.json`.

## 1. Build the graph

```bash
hexa graph build              # (re)build graph-out/graph.json for this project
```

Every graph verb reads the saved file, so run this first, and again after the
code changes. A stale graph answers about the code as it was.

## 2. Ask the graph

| Need | Command |
|---|---|
| One file, ready to paste into a prompt: defines, uses, consumers, community | `hexa graph context <file>` (`--json` for the raw bundle, `--max-each <n>` to cap each list) |
| Who depends on a file before you change or delete it | `hexa graph consumers <file>` |
| The project at a glance: layers, grade, what is wrong | `hexa analyze .` |

## 3. Report

Say which files you asked about and paste the `hexa graph context` output, or
the part of it that answers the question. Do not summarize a file you have not
asked the graph or read.
