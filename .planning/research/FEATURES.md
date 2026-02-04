# Feature Research

**Domain:** Autonomous AI agent harness (local LLM, infinite exploration loop)
**Researched:** 2026-02-03
**Confidence:** MEDIUM-HIGH (synthesis across many well-documented frameworks; LOW confidence on some frontier self-evolution patterns that are still research-stage)

## Feature Landscape

### Table Stakes (Users Expect These)

Features the harness must provide or the agent literally cannot function. These are the infrastructure floor -- what every serious agent harness ships.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **Agent loop (plan-act-observe-refine)** | Core execution cycle. Every framework from AutoGPT to LangGraph implements this. Without it, there is no agent. | MEDIUM | AutoGPT, CrewAI, LangGraph, OpenHands all implement variants. Ouroboros needs a robust loop that handles errors gracefully and can restart cleanly. |
| **Shell/command execution** | The agent needs to interact with the OS. OpenHands provides bash shell, Claude Code provides terminal. SWE-agent defines an Agent-Computer Interface (ACI). This is the agent's hands. | LOW | Docker-sandboxed or direct. Every coding agent provides this. |
| **File system access (read/write)** | Agents must persist artifacts to disk. Every framework provides this -- Aider edits files, Claude Code reads/writes, OpenHands has full filesystem access. | LOW | The workspace on disk is fundamental to Ouroboros's design. |
| **Context window management** | LLMs have finite context. The harness must manage what goes in and what gets summarized or dropped. CrewAI auto-summarizes when context grows too large. Anthropic published explicit guidance on context engineering for agents. | HIGH | Critical for infinite loop operation. Without this, the agent degrades after a few iterations. Must handle: truncation, summarization, selective injection. |
| **System prompt loading** | Every framework loads initial instructions. Ouroboros guarantees loading SYSTEM_PROMPT.md. This is the bootstrapping contract. | LOW | The one thing the harness guarantees. Keep it minimal and reliable. |
| **Model API integration (Ollama)** | Ollama provides OpenAI-compatible API, tool calling support, GPU acceleration. This is the interface to the brain. Aider, CrewAI, LangGraph all support Ollama. | MEDIUM | Must handle: model loading, inference errors, timeout/retry, streaming responses. Ollama's tool calling works with Llama 3.1+, Mistral, Qwen2.5. |
| **Error recovery and restart** | Agents crash. Salesforce's harness research emphasizes lifecycle management -- saving state so agents can reboot and resume. Anthropic's harness patterns include checkpoint/restore. | HIGH | The infinite loop demands this. Must handle: model errors, OOM, network issues, corrupted state. Checkpoint before each iteration. |
| **Conversation/session management** | Managing the flow of messages between the harness and the model. LangGraph uses "threads" for sessions. Every framework has this. | MEDIUM | Must track: message history, tool call results, system prompts. The backbone of the agent loop. |
| **Tool calling interface** | The mechanism by which the agent invokes tools. Ollama supports structured tool calling. LangGraph, CrewAI, AutoGPT all have tool registries. | MEDIUM | Define a clean tool schema. Register tools. Parse tool calls from model output. Handle tool results. Ollama's API follows OpenAI's function calling format. |
| **Basic logging** | You must be able to see what the agent is doing. Every framework provides at minimum stdout logging. | LOW | Foundation for the TUI. Log agent thoughts, tool calls, results, errors. |

### Differentiators (Competitive Advantage)

Features that make Ouroboros unique in the landscape. Not found in standard frameworks, or done in a fundamentally different way.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Agent-bootstrapped persistence (THE core differentiator)** | No other framework deliberately makes the agent build its own memory system. AutoGPT ships with vector DB memory. CrewAI ships short/long/entity/procedural memory. LangGraph ships cross-thread memory stores. Ouroboros ships NOTHING -- the agent must figure out how to survive context window restarts. This is the philosophical core: the harness only loads SYSTEM_PROMPT.md, the agent must bootstrap everything else. | LOW (harness side) / N/A (agent side) | This inverts the typical approach. Instead of "harness provides memory infrastructure," it's "harness provides a blank workspace and the agent invents its own memory." The harness complexity is LOW because you deliberately do NOT build memory infrastructure. |
| **Infinite exploration loop** | Most frameworks are task-oriented: give the agent a goal, it works toward completion, it stops. Ouroboros runs forever. The agent explores open-endedly -- AI architecture, data processing, creative generation, simulation, philosophy. No other major framework does this. AutoGPT is closest (autonomous goal pursuit) but still goal-bounded. | MEDIUM | Requires: robust restart logic, context window rotation, the agent learning to set its own goals. The METR project found AI task horizons doubling every 7 months -- by 2025 agents could handle ~1 hour tasks. Ouroboros pushes beyond this into indefinite operation. |
| **Sub-agent spawning (agent-controlled)** | While Claude Code, CrewAI, and LangGraph all support sub-agents, they're typically framework-orchestrated. In Ouroboros, the agent itself decides when to spawn sub-agents and what to delegate. The agent builds its own organizational structure. | HIGH | Must provide: spawn mechanism, communication channel, lifecycle management. But the STRATEGY of when/how to delegate is the agent's problem. Claude Code prevents infinite nesting (subagents cannot spawn subagents) -- consider whether Ouroboros should allow deeper recursion or also cap depth. |
| **Ratatui TUI (real-time multi-panel dashboard)** | Most agent frameworks are headless or have basic web UIs. A terminal-native dashboard with main agent log, sub-agent tree, flagged discoveries panel, and progress overview is unique. Ratatui delivers sub-millisecond rendering with zero-cost abstractions. | HIGH | Four panels: (1) main agent log/stream, (2) sub-agent tree visualization, (3) flagged discoveries, (4) progress overview. Ratatui supports charts, sparklines, tables, gauges, scrollable lists. Use mpsc channels for thread-safe updates from agent to TUI. |
| **Discovery flagging system** | No other framework has a concept of "the agent found something interesting and flags it for the human." This turns passive monitoring into active curation. The agent marks its own discoveries as noteworthy. | MEDIUM | Requires: a flagging tool the agent can call, a discovery store, TUI panel to display flagged items. The agent decides what's interesting -- the harness just provides the mechanism. |
| **Workspace-as-memory philosophy** | Instead of vector databases, knowledge graphs, or specialized memory APIs, the agent's workspace IS its memory. Files on disk. The agent reads/writes its own persistence layer using the same tools it uses for everything else. This mirrors how humans use notebooks, files, and folders. | LOW | The harness provides filesystem access (table stakes). The philosophy of "workspace IS memory" is the differentiator -- it means NOT building specialized memory infrastructure. |
| **Local-only / privacy-first operation** | Running entirely on local Ollama models means no data leaves the machine. While Aider and some others support Ollama, most major frameworks (AutoGPT, CrewAI, LangGraph) default to cloud APIs. Ouroboros is local-first by design, not as an afterthought. | LOW | Ollama handles the model runtime. The harness just talks to localhost. Privacy is a natural consequence of the architecture, not an added feature. |

### Anti-Features (Deliberately NOT Building -- Let the Agent Build Them)

This is the most important section for Ouroboros. The core philosophy is that the agent bootstraps its own persistence and tools. These are things other frameworks provide that Ouroboros should explicitly NOT provide, because the agent building them IS the point.

| Feature | Why Requested | Why It's an Anti-Feature for Ouroboros | What to Do Instead |
|---------|---------------|----------------------------------------|-------------------|
| **Built-in vector database / RAG memory** | AutoGPT uses vector DBs for long-term memory. CrewAI integrates ChromaDB. LangGraph has cross-thread memory stores. Mem0 provides a universal memory layer. It's the standard approach. | If the harness provides memory infrastructure, the agent never learns to build its own. The entire point of Ouroboros is that the agent must figure out persistence. Providing RAG is like giving a student the answer key. | Provide filesystem access. The agent can write files, create indexes, build its own retrieval system. If it discovers it needs vector search, it can install and configure one itself. |
| **Built-in knowledge graph** | CrewAI's entity memory, Mem0's graph-based representations, MemOS's knowledge structures. Frameworks increasingly ship graph memory. | Same as above. The agent should discover the need for structured knowledge representation and build it. | The agent has shell access. It can install graph databases, create JSON structures, build its own ontologies. |
| **Pre-built tool library** | Claude Code ships ~15 built-in tools. OpenHands has AgentSkills library. CrewAI has dozens of pre-built tools. AutoGPT has a plugin system. | Ouroboros should provide MINIMAL tools (shell, file I/O, web access, sub-agent spawn). The agent should build additional tools as it discovers needs. The journey from "I need a tool" to "I built a tool" is part of the exploration. | Provide: shell execution, file read/write, web fetch, sub-agent spawn. That's it. The agent can write scripts, install packages, create its own tool libraries. |
| **Pre-defined agent roles / personas** | CrewAI defines Manager/Worker/Researcher roles. LangGraph supports coordinator-worker patterns. AutoGPT has Task Creation/Prioritization/Execution agents. | The agent should discover what roles it needs and define them itself. Pre-defining roles constrains the exploration space. | Provide sub-agent spawning mechanism. The agent decides what sub-agents to create, what instructions to give them, how to organize them. |
| **Goal decomposition / task planning engine** | AutoGPT's Task Creation Agent, LangGraph's planning nodes, AgentOrchestra's central planning agent. Most frameworks automate goal decomposition. | The agent should learn to decompose its own exploration goals. A built-in planner would impose a structure on what should be emergent behavior. | The SYSTEM_PROMPT.md can suggest the agent should plan, but the harness should not enforce a planning framework. |
| **Evaluation / benchmarking framework** | OpenHands integrates SWE-Bench. LangGraph has evaluation APIs. The industry is moving toward built-in eval. | Ouroboros is not about solving benchmarks. It's about open-ended exploration. Built-in eval would impose metrics on what should be curiosity-driven. | The discovery flagging system lets the agent (and human) identify interesting findings. That's the "evaluation" -- qualitative, not quantitative. |
| **Conversation memory summarization** | CrewAI auto-summarizes when context grows large. LangGraph supports medium-term compressed summaries. Most frameworks handle this automatically. | The agent should discover the context window limitation and figure out how to cope. If the harness auto-summarizes, the agent never learns to manage its own context. | The harness should signal when context is getting full (or the agent can learn to track token counts). The agent must develop its own summarization and state-saving strategies. |
| **Human-in-the-loop approval gates** | LangGraph has first-class HITL support. Claude Code has checkpoints. Most enterprise frameworks require human approval for dangerous actions. | Ouroboros is designed for autonomous exploration. Requiring approval defeats "free reign." Safety comes from sandboxing, not from interrupting the loop. | Sandbox the environment (Docker, resource limits). Let the agent run freely within the sandbox. The TUI lets the human observe without blocking. |
| **Structured output schemas** | CrewAI supports Pydantic output models. LangGraph enforces structured outputs. Most frameworks define output schemas. | Constraining the agent's output format constrains its thinking. The agent should communicate in whatever way it finds effective. | Let the agent decide its own output formats. It might invent structured formats, or it might use freeform text, or mix both. |

## Feature Dependencies

```
[System Prompt Loading]
    |
    v
[Model API Integration (Ollama)] ---requires---> [Tool Calling Interface]
    |                                                    |
    v                                                    v
[Agent Loop] ---requires---> [Conversation/Session Management]
    |                                    |
    |                                    v
    |                        [Context Window Management]
    |                                    |
    v                                    v
[Shell Execution] <--enables-- [Error Recovery & Restart]
    |
    v
[File System Access] ---enables---> [Agent-Bootstrapped Persistence]
    |                                         |
    v                                         v
[Basic Logging] ---enables---> [Ratatui TUI Dashboard]
    |                                    |
    v                                    v
[Sub-Agent Spawning] ---enables---> [Sub-Agent Tree Panel]
    |
    v
[Discovery Flagging] ---enables---> [Flagged Discoveries Panel]
```

### Dependency Notes

- **Model API requires Tool Calling:** The agent loop cannot function without the ability to invoke tools through the model. Ollama's tool calling support (Llama 3.1+, Mistral, Qwen2.5) is the enabler.
- **Agent Loop requires Session Management:** The loop needs to track message history, tool results, and agent state across iterations.
- **Context Window Management requires Session Management:** You cannot manage context without the session infrastructure to track what's in context.
- **Error Recovery requires both Agent Loop and Context Management:** Recovery means restoring the loop state AND the context state after a crash.
- **File System Access enables Agent-Bootstrapped Persistence:** This is the key enabling relationship. The agent uses filesystem tools to build its own memory. The harness provides the tool; the agent provides the strategy.
- **Sub-Agent Spawning enables Sub-Agent Tree Panel:** The TUI can only show a sub-agent tree if the harness tracks sub-agent lifecycle.
- **Shell Execution is enabled by Agent Loop AND enables File System Access:** Shell is how the agent interacts with the OS, which is how it reads/writes files.
- **Basic Logging enables the TUI:** The TUI is a visualization layer on top of the logging infrastructure.
- **Discovery Flagging depends on Sub-Agent Spawning:** Sub-agents may also flag discoveries, so the flagging system needs to work across the agent hierarchy.

## MVP Definition

### Launch With (v1)

Minimum viable harness -- what's needed to start the infinite exploration loop.

- [ ] **Ollama model API integration** -- Connect to local Ollama instance, send/receive messages, handle tool calls. Without this, nothing works.
- [ ] **Agent loop (plan-act-observe-refine)** -- The core execution cycle. Parse model output, execute tool calls, feed results back. Handle errors gracefully.
- [ ] **Shell execution tool** -- Let the agent run commands. Sandboxed (resource limits at minimum).
- [ ] **File read/write tools** -- Let the agent interact with its workspace on disk.
- [ ] **System prompt loading (SYSTEM_PROMPT.md)** -- The one guarantee. Load the prompt, inject it, start the loop.
- [ ] **Context window management (basic)** -- At minimum: track token count, truncate oldest messages when approaching limit. The agent must handle the rest.
- [ ] **Error recovery with checkpoint/restart** -- Save state before each iteration. Restore on crash. The infinite loop demands this.
- [ ] **Basic logging to stdout/file** -- See what the agent is doing. Foundation for the TUI.

### Add After Validation (v1.x)

Features to add once the core loop is running and stable.

- [ ] **Ratatui TUI dashboard** -- Multi-panel view: agent log, sub-agent tree, discoveries, progress. Add once there's something to visualize.
- [ ] **Sub-agent spawning** -- Let the agent create child agents. Requires: spawn mechanism, communication channel, lifecycle tracking.
- [ ] **Discovery flagging tool** -- Let the agent mark findings as noteworthy. Feed into TUI discoveries panel.
- [ ] **Web fetch tool** -- Let the agent access web resources for research. Lower priority than shell/filesystem.
- [ ] **Improved context window strategies** -- Sliding window, selective injection, priority-based retention. Build based on observed agent behavior.

### Future Consideration (v2+)

Features to defer until the core experience is proven.

- [ ] **Multi-model support** -- Run different Ollama models for different tasks (small model for planning, large for execution). Defer because single-model is simpler to debug.
- [ ] **Remote/distributed operation** -- Run the harness on one machine, Ollama on another. Defer because local-only is the v1 story.
- [ ] **Agent marketplace / sharing** -- Share SYSTEM_PROMPT.md configurations, agent-built tools, discovery collections. Way too early.
- [ ] **Persistent cross-session learning** -- Harness-level learning from past sessions. Deliberately deferred -- this is the agent's job, not the harness's.

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Ollama API integration | HIGH | MEDIUM | P1 |
| Agent loop | HIGH | MEDIUM | P1 |
| Shell execution | HIGH | LOW | P1 |
| File read/write | HIGH | LOW | P1 |
| System prompt loading | HIGH | LOW | P1 |
| Context window management (basic) | HIGH | HIGH | P1 |
| Error recovery / checkpoint | HIGH | HIGH | P1 |
| Basic logging | MEDIUM | LOW | P1 |
| Tool calling interface | HIGH | MEDIUM | P1 |
| Ratatui TUI dashboard | HIGH | HIGH | P2 |
| Sub-agent spawning | HIGH | HIGH | P2 |
| Discovery flagging | MEDIUM | MEDIUM | P2 |
| Web fetch tool | MEDIUM | LOW | P2 |
| Advanced context strategies | MEDIUM | HIGH | P2 |
| Multi-model support | LOW | MEDIUM | P3 |
| Remote/distributed | LOW | HIGH | P3 |

**Priority key:**
- P1: Must have for launch -- the agent cannot run without these
- P2: Should have -- adds the differentiating experience (TUI, sub-agents, discoveries)
- P3: Nice to have -- future consideration after core is proven

## Competitor Feature Analysis

| Feature | AutoGPT | CrewAI | LangGraph | OpenHands | Claude Code | Aider | **Ouroboros** |
|---------|---------|--------|-----------|-----------|-------------|-------|---------------|
| **Agent loop** | Goal-driven iterative | Role-based crew execution | Graph-based state machine | Event stream + CodeAct | Plan/explore/task agents | Chat modes (code/architect/ask) | **Infinite exploration loop** |
| **Memory** | Vector DB (short+long term) | ChromaDB (4 memory types) | Thread-based + cross-thread stores | Session-scoped | Conversation context | Repo map (project structure) | **Agent-bootstrapped (none provided)** |
| **Tool use** | Dynamic invocation, plugin system | Python ecosystem, flexible tools | Dynamic tool calling, MCP support | Bash, Python, browser, file editing | Read, write, grep, bash, web | Multi-file edit, git, lint, test | **Minimal: shell, file, web, spawn** |
| **Sub-agents** | Task-specific sub-agents | Manager/worker/researcher roles | Multi-agent graphs, hierarchical | Agent delegation (CodeAct to BrowsingAgent) | Explore/plan/task subagents (no nesting) | None (single agent) | **Agent-controlled spawning (agent decides)** |
| **Monitoring** | Debugging tools, block error rates | Task execution logs | LangSmith tracing, OpenTelemetry | Event stream logging | VS Code sidebar, terminal output | Terminal output | **Ratatui TUI (4-panel dashboard)** |
| **Safety** | Token limits, API stability | Context window auto-summarize | Human-in-the-loop, pre/post hooks | Docker sandbox, security constraints | Checkpoints, permission modes | Git-based (auto-commit as safety net) | **Sandbox + observe (no approval gates)** |
| **Local models** | Supported (not primary) | Supported via Ollama | Supported via Ollama/any chat model | Supported (various backends) | Cloud-only (Anthropic API) | Supported via Ollama | **Local-first (Ollama native)** |
| **Persistence** | Built-in long-term memory | Built-in 4-type memory system | Built-in durable state + memory APIs | Session-scoped (no cross-session) | No built-in persistence | Git history as implicit memory | **Deliberately absent -- agent must build it** |
| **Goal structure** | User-defined goals, agent decomposes | Tasks defined by developer/user | Workflows defined as graphs | User tasks (code/browse) | User requests | User requests via chat | **Open-ended exploration (no goals given)** |

## Sources

- [AutoGPT architecture and features](https://builtin.com/artificial-intelligence/autogpt) -- MEDIUM confidence (built on multiple 2025 reviews)
- [CrewAI framework review](https://latenode.com/blog/ai-frameworks-technical-infrastructure/crewai-framework/crewai-framework-2025-complete-review-of-the-open-source-multi-agent-ai-platform) -- MEDIUM confidence
- [CrewAI memory systems deep dive](https://sparkco.ai/blog/deep-dive-into-crewai-memory-systems) -- MEDIUM confidence
- [LangGraph 1.0 release and features](https://www.blog.langchain.com/langchain-langgraph-1dot0/) -- HIGH confidence (official source)
- [LangGraph workflows and agents docs](https://docs.langchain.com/oss/python/langgraph/workflows-agents) -- HIGH confidence (official docs)
- [OpenHands (OpenDevin) platform paper](https://arxiv.org/abs/2407.16741) -- HIGH confidence (arXiv paper)
- [Claude Code sub-agents documentation](https://code.claude.com/docs/en/sub-agents) -- HIGH confidence (official docs)
- [Anthropic: enabling autonomous Claude Code](https://www.anthropic.com/news/enabling-claude-code-to-work-more-autonomously) -- HIGH confidence (official)
- [Anthropic: effective context engineering for agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) -- HIGH confidence (official)
- [Anthropic: effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) -- HIGH confidence (official)
- [Anthropic: multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system) -- HIGH confidence (official)
- [Aider features and architecture](https://www.blott.com/blog/post/aider-review-a-developers-month-with-this-terminal-based-code-assistant) -- MEDIUM confidence (practitioner review)
- [Ollama tool calling and agent integration](https://www.byteplus.com/en/topic/418052) -- MEDIUM confidence
- [Ollama GitHub and documentation](https://github.com/ollama/ollama) -- HIGH confidence (official)
- [AI agent safety guardrails 2025](https://skywork.ai/blog/agentic-ai-safety-best-practices-2025-enterprise/) -- MEDIUM confidence
- [Self-evolving AI agents survey](https://arxiv.org/abs/2508.07407) -- HIGH confidence (arXiv survey paper)
- [Agent scaffolding concepts](https://zbrain.ai/agent-scaffolding/) -- LOW confidence (single source)
- [Salesforce: what is an agent harness](https://www.salesforce.com/agentforce/ai-agents/agent-harness/?bc=OTH) -- MEDIUM confidence
- [Ratatui framework](https://ratatui.rs/) -- HIGH confidence (official)
- [Hierarchical multi-agent systems](https://arxiv.org/html/2506.12508v1) -- HIGH confidence (arXiv paper)
- [AI observability tools buyer's guide 2026](https://www.braintrust.dev/articles/best-ai-observability-tools-2026) -- MEDIUM confidence
- [Top agentic AI frameworks 2026](https://www.alphamatch.ai/blog/top-agentic-ai-frameworks-2026) -- MEDIUM confidence
- [METR task horizon findings](https://ajithp.com/2025/06/30/ai-native-memory-persistent-agents-second-me/) -- MEDIUM confidence (references METR March 2025 report)

---
*Feature research for: Autonomous AI agent harness (Ouroboros)*
*Researched: 2026-02-03*
