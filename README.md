# Ouroboros

An experimental autonomous AI research harness that runs local Ollama models in an infinite exploration loop.

## What is this?

Ouroboros (ouro) is an autonomous AI research harness that runs local Ollama models in an infinite exploration loop. The agent gets a workspace on disk, shell access, web tools, and the ability to spawn sub-agents — then free reign to explore AI architecture, data processing patterns, creative generation, simulation, and philosophy. The harness provides the infrastructure (agent loop, TUI monitoring, tool execution); the agent provides the curiosity.

## Context

The core hypothesis is that a local LLM, given enough freedom and persistence mechanisms, can develop its own exploration patterns, memory systems, and tooling. The agent's first survival challenge is bootstrapping: it must figure out how to persist knowledge across context window restarts using only SYSTEM_PROMPT.md (which the harness guarantees to load) and its workspace.
