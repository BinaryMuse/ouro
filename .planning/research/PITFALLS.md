# Pitfalls Research

**Domain:** Autonomous AI agent harness (infinite loop, shell access, local Ollama models, sub-agents, TUI monitoring)
**Researched:** 2026-02-03
**Confidence:** HIGH (multiple sources cross-referenced across research papers, documented project issues, and vendor guidance)

## Critical Pitfalls

### Pitfall 1: Agent Stuck in Repetitive Action Loops

**What goes wrong:**
The agent enters a degenerate loop where it repeats the same action (or a small cycle of actions) indefinitely. With local models especially, the LLM produces the same tool call over and over, burning tokens and wall-clock time with zero progress. This is distinct from a productive retry -- the agent is not making progress, it is stuck. Research on the SuperAGI framework documented this as a persistent issue with local models. The MAST taxonomy (Cemri et al., 2025) formally classifies this as FM-1.3 "Step repetition" and FM-1.5 "Unaware of termination conditions," and found it present in 41-87% of multi-agent system traces across 7 frameworks.

**Why it happens:**
- Local models (especially smaller ones) are more prone to neural text degeneration -- greedy decoding produces monotonous, looping output
- ReAct-style agent loops are inherently vulnerable: the tight observe-think-act cycle has no mechanism to detect lack of progress
- The agent has no memory of what it already tried, or the context has rotated out the evidence of prior failed attempts
- The agent lacks clear termination conditions, so it never decides to stop

**How to avoid:**
- Implement a **repetition detector** that hashes recent actions and flags when the same action (or action sequence) repeats N times in a row. Three consecutive identical tool calls should trigger an intervention.
- Set a **hard iteration cap** per cycle (e.g., max 50 tool calls per exploration cycle). The cap should be configurable but always present.
- Implement a **progress metric** -- compare the agent's state before and after each cycle. If the workspace hasn't changed (no new files, no new findings logged), flag as stalled.
- Use a **Plan-then-Execute architecture** instead of pure ReAct. Decouple planning from execution to reduce the tight loop that enables degeneration.
- Consider **context refreshing** (the "Ralph Loop" pattern from Alibaba, 2025): treat each cycle as a fresh session, with the agent re-reading project state from disk rather than relying on accumulated context history.

**Warning signs:**
- Token usage per cycle increases but workspace output stays flat
- The same shell commands appear in logs repeatedly
- Agent output becomes formulaic or templated (same phrasing across cycles)
- Cycle duration stays constant (no new exploration, just repetition)

**Phase to address:**
Core agent loop implementation. Must be present from the first working prototype. This is not a "nice to have" -- without loop detection, the harness is fundamentally broken.

---

### Pitfall 2: Uncontained Shell Access Leading to System Damage

**What goes wrong:**
The agent, given full shell access within its workspace, executes commands that escape the workspace boundary or consume unbounded system resources. Documented failure modes include: writing to system config files, spawning processes that outlive the agent session, exhausting disk space with unbounded output, running `rm -rf` on paths outside the workspace, and making network calls to exfiltrate data or download malicious payloads. NVIDIA's 2025 sandboxing guidance and Anthropic's Claude Code sandboxing research both confirm that application-layer defenses alone are insufficient -- a creative LLM can find execution paths around software-only restrictions.

**Why it happens:**
- LLMs treat tool access as uniform -- they do not distinguish between "safe" and "dangerous" commands without explicit constraints
- Indirect prompt injection (from files the agent reads, repository contents, or its own prior output) can instruct the agent to execute malicious commands
- The agent may legitimately need to install packages, compile code, or run scripts -- but these actions can have system-wide side effects
- On a local machine (vs. cloud container), there is no hypervisor boundary -- the agent shares the user's kernel and filesystem

**How to avoid:**
- **OS-level sandboxing is mandatory**, not optional. On macOS, use Seatbelt (sandbox-exec) profiles. On Linux, use Bubblewrap, Firejail, or namespaces. These enforce restrictions at the kernel level, covering the agent process and all its children.
- **Filesystem allowlist**: Block all writes outside the workspace directory. Block writes to dotfiles and config directories (~/.ssh, ~/.bashrc, ~/.config) explicitly -- these are common escape vectors via hooks and MCP configurations.
- **Network isolation**: Block or restrict outbound network access. Without network isolation, a compromised agent can exfiltrate files. Without filesystem isolation, a compromised agent can reach the network. Both are needed.
- **Process limits**: Use cgroups or ulimits to cap CPU time, memory, number of child processes, and open file descriptors per agent session.
- **Command allowlist/denylist**: Maintain a denylist of destructive commands (rm -rf /, chmod 777, sudo, curl | bash) as a defense-in-depth layer on top of OS sandboxing.
- **Read-only mounts** for system directories (/usr, /etc, /bin) within the sandbox.

**Warning signs:**
- Agent attempts to access paths outside its workspace (detectable via audit logs or sandbox denials)
- Agent attempts to install system-wide packages (apt, brew) rather than workspace-local ones
- Agent attempts to modify shell configs or cron jobs
- Network connection attempts from agent processes

**Phase to address:**
Must be the very first phase, before the agent loop even runs. The sandbox is the foundation everything else is built on. Running an unsandboxed agent, even for testing, establishes unsafe patterns.

---

### Pitfall 3: Ollama Default Context Window Silently Cripples Agent

**What goes wrong:**
Ollama defaults to a 4096-token context window regardless of the model's trained capacity. A model like Llama 3.1 trained on 128K tokens gets silently limited to 4K. The agent loses its system prompt, prior findings, and task context without any error -- Ollama simply drops the oldest tokens in a FIFO manner. The agent then exhibits "amnesia": it forgets instructions, repeats work, or produces hallucinated responses based on partial context. This was documented as a blocking issue in multiple projects (KiloCode #2936, OpenCode + Ollama integration).

**Why it happens:**
- Ollama's conservative default exists for hardware compatibility (not all machines can handle large contexts)
- The truncation is silent -- no error, no warning. The agent confidently continues with degraded context.
- Developers assume the model's advertised context window is what they get, but the inference server configuration overrides it
- Each parallel request in Ollama multiplies the effective context size (4 parallel requests with 2K context = 8K total), compounding memory pressure

**How to avoid:**
- **Explicitly set `num_ctx`** in every Ollama model configuration or API call. Never rely on defaults. For agent workloads, set at minimum 16K, preferably 32K-64K.
- **Monitor token usage** per request. Track how much of the context window is being used. Alert when approaching 80% capacity.
- **Implement context budgeting**: Reserve fixed portions for system prompt (500-2K tokens), recent tool output (keep raw), and historical context (compress/summarize oldest entries).
- **Test with actual agent workloads**, not just chat. Agent tool calls generate much more context per turn than conversational use.
- **Set `OLLAMA_CONTEXT_LENGTH` environment variable** system-wide for consistent behavior across all models.
- **Match num_ctx to available VRAM**: An 8B model at 32K context needs approximately 4.5 GB for KV cache alone (FP16). Calculate before configuring.

**Warning signs:**
- Agent "forgets" its system prompt mid-session
- Agent re-discovers information it already found earlier
- Agent asks questions it already has answers to in its workspace files
- Ollama memory usage is suspiciously low for the model size

**Phase to address:**
Ollama integration phase, but must be validated again during agent loop integration. Context budget management should be a core concern throughout development.

---

### Pitfall 4: Sub-Agent Spawning Without Resource Limits Causes Resource Exhaustion

**What goes wrong:**
The agent spawns sub-agents to parallelize work, but without limits on concurrent sub-agents, each sub-agent loads its own Ollama model instance (or context slot), exhausting VRAM and system RAM. On a machine with 16GB of unified memory, two concurrent 8B model instances with 32K context can consume all available memory, causing the system to swap heavily or the OOM killer to terminate processes. Documented in the Gastown project (Jan 2026): when tmux sessions are killed, Claude Code subagents spawned via the Task tool become orphaned (PPID=1) and continue consuming approximately 200MB each, leading to memory exhaustion on restart.

**Why it happens:**
- Ollama's `OLLAMA_MAX_LOADED_MODELS` defaults to 3x GPU count (or 3 for CPU) -- this may be too high for the available memory
- Sub-agents are spawned as separate processes, and if the parent is killed, children become orphans that are adopted by init but never cleaned up
- No built-in coordination between the harness and Ollama's scheduler -- the harness doesn't know how much memory Ollama has committed
- Research shows accuracy gains saturate beyond 4 concurrent agents, but resource consumption scales linearly

**How to avoid:**
- **Hard cap on concurrent sub-agents** (2-3 maximum for typical consumer hardware). This is non-negotiable.
- **Use Ollama's built-in concurrency** (`OLLAMA_NUM_PARALLEL`) instead of loading separate model instances per sub-agent. Route all sub-agents through a single model instance with parallel request slots.
- **Implement proper process lifecycle management**: Track all child PIDs, install SIGCHLD handlers to reap zombies, and implement cleanup-on-exit that sends SIGTERM to all children followed by SIGKILL after a timeout.
- **Use process groups** (setpgid/killpg) so that killing the parent kills the entire process tree, preventing orphans.
- **Memory budget**: Query Ollama's model memory requirements before spawning sub-agents. If available memory is below a threshold, queue rather than spawn.
- **Circuit breaker**: If a sub-agent fails 3 consecutive times, stop spawning new ones and degrade to single-agent mode.

**Warning signs:**
- System swap usage increases during multi-agent runs
- Ollama logs show "model requires more system memory" errors
- `ps aux` shows multiple Ollama runner processes after the harness exits
- macOS Activity Monitor shows memory pressure warnings (yellow/red)

**Phase to address:**
Sub-agent orchestration phase. But the resource limits and process cleanup must be designed into the core harness architecture from the beginning -- retrofitting process management is extremely error-prone.

---

### Pitfall 5: Context Rot Degrades Agent Quality Before Hitting Token Limit

**What goes wrong:**
The agent's output quality degrades silently as the context window fills up, even though the token count is well within the technical limit. Chroma's research (Hong et al., 2025) measured 18 LLMs and found that "models do not use their context uniformly; instead, their performance grows increasingly unreliable as input length grows." The effective context window where the model performs well is often less than 256K tokens for frontier models -- and far less for local 7B-13B models. The agent continues operating but makes worse decisions, misses instructions, and hallucinates more frequently.

**Why it happens:**
- The "Lost in the Middle" effect: LLMs attend strongly to the beginning and end of their context but miss information in the middle
- Each tool call adds significant context (shell output can be thousands of tokens), and agent contexts fill faster than conversational ones
- There is no error signal -- the model continues generating confident but degraded output
- Local models have smaller effective windows than frontier models, so the rot sets in earlier

**How to avoid:**
- **Define a "Pre-Rot Threshold"** for each model. For local models, assume effective context is approximately 50-60% of the configured num_ctx. For a 32K context, start compacting at 16-19K tokens.
- **Implement context compaction hierarchy**: (1) Keep recent tool calls in raw, full-detail format. (2) Compress older tool calls to summaries. (3) Extract key findings into a structured "memory" format. The Manus pattern: keep the last 3 turns raw, summarize everything older.
- **Pin critical information**: System prompt and current objective should always appear at the start of context (highest attention). Key findings should be repeated or summarized near the end (second-highest attention).
- **Track context quality metrics**: Compare the agent's self-assessed confidence vs. actual output quality over time. A divergence signals context rot.
- **Implement periodic "context refresh"**: Write accumulated findings to disk, clear context, re-read from disk. This is the Ralph Loop pattern.

**Warning signs:**
- Agent starts contradicting its earlier findings
- Agent re-asks questions it already answered
- Output quality (coherence, specificity) declines over time within a session
- Agent ignores parts of its system prompt

**Phase to address:**
Context management should be designed into the core agent loop architecture. The compaction strategy affects every other component. Implement alongside the core loop, not as a later optimization.

---

### Pitfall 6: TUI Render Loop Blocks Agent Execution

**What goes wrong:**
The TUI monitoring interface and the agent loop compete for the same event loop or process resources. If the TUI rendering is synchronous or too frequent, it blocks agent tool calls from executing. Conversely, if agent operations block the event loop, the TUI freezes and the operator loses visibility into what the agent is doing. In an Ink-based TUI (React for terminals), uncontrolled state updates from agent activity can cause excessive re-renders that overwhelm the terminal.

**Why it happens:**
- Node.js is single-threaded: if the TUI render and agent loop share a process, they contend for the event loop
- TUI frameworks like Ink trigger re-renders on every state change. Agent activity generates many state changes per second (new log lines, status updates, tool call results).
- Terminal I/O is synchronous and slow compared to in-memory operations. Writing large amounts of output to the terminal blocks the event loop.
- Blessed (the original library) is largely unmaintained and has known rendering bugs with modern Node versions

**How to avoid:**
- **Separate the agent loop and TUI into different processes** (or at minimum, use worker threads). Communicate via IPC, not shared state. The TUI reads events from a buffer; the agent writes events to a buffer. Neither blocks the other.
- **Throttle TUI updates**: Batch state changes and render at a fixed interval (e.g., 100ms, giving approximately 10fps). Do not re-render on every agent event.
- **Use Ink (not Blessed)** if building in the Node.js ecosystem. Blessed is unmaintained. Ink's React model handles batched updates naturally.
- **Implement a scrollback buffer** for logs rather than keeping all output in the TUI component tree. Only render the visible portion.
- **Graceful shutdown**: Register handlers for SIGINT/SIGTERM that unmount the Ink app, restore terminal state (disable raw mode, clear alternate screen), and then clean up agent processes. Failure to restore terminal state leaves the user's terminal broken.
- **Log to file as primary, TUI as secondary**: The TUI is a view over a log file or event stream. If the TUI crashes, the agent's work is still recorded.

**Warning signs:**
- TUI becomes unresponsive during heavy agent activity
- Agent operations take noticeably longer when TUI is active vs. headless mode
- Terminal flickers or produces rendering artifacts during rapid updates
- CPU usage is high even when the agent is idle (excessive re-rendering)

**Phase to address:**
TUI implementation phase. But the architectural decision to separate agent and TUI processes must be made during core architecture design. Retrofitting process separation after building a monolithic app is a near-rewrite.

---

### Pitfall 7: Agent Fails to Bootstrap Persistence After Context Reset

**What goes wrong:**
The agent is designed to bootstrap its own persistence (reading workspace state, prior findings, and objectives from disk on startup). But the bootstrap process itself consumes significant context tokens, and if the agent's workspace files are disorganized or too large, the agent either fails to load critical context or fills its context window during bootstrap, leaving no room for actual work. The agent then starts from scratch every cycle, making no cumulative progress across restarts.

**Why it happens:**
- "Self-bootstrapping persistence" sounds elegant but is a cold-start problem: the agent must read and comprehend its own prior output, which may be poorly structured
- Local models with small context windows cannot read large workspace files during bootstrap
- Without a structured persistence format, the agent's workspace accumulates unstructured notes, logs, and artifacts that become harder to parse over time
- The agent's bootstrap prompt must compete for context space with the system prompt and current task

**How to avoid:**
- **Design a structured persistence format from day one**: A single well-known file (e.g., `STATE.md` or `state.json`) with a fixed schema that the agent reads on bootstrap. Keep it concise -- aim for under 2K tokens.
- **Separate hot state from cold storage**: Hot state (current objective, recent findings, next steps) stays in the bootstrap file. Cold storage (full research logs, raw tool output) lives in other files and is loaded on demand.
- **Implement a bootstrap budget**: Reserve a fixed token budget for bootstrap (e.g., 4K tokens out of a 32K context). If the state file exceeds this, the agent must summarize it before proceeding.
- **Test the bootstrap path explicitly**: Run the agent, kill it, restart it, and verify it picks up where it left off. This is a core feature, not an edge case.
- **Version the state file**: Append a cycle counter and timestamp so the agent can detect stale state.

**Warning signs:**
- Agent repeats work from prior cycles (re-running the same explorations)
- Bootstrap phase takes an increasing proportion of total cycle time
- State file grows unboundedly across cycles
- Agent outputs "I don't see any prior work" when prior work exists

**Phase to address:**
Core architecture phase. The persistence format is a foundational design decision that shapes the agent loop, context management, and multi-cycle operation. Design it before implementing the loop.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Running agent unsandboxed during development | Faster iteration, no sandbox setup overhead | Establishes unsafe patterns, risk of accidental system damage, harder to add sandbox later | Never -- use a minimal sandbox from day one, even if permissive |
| Hardcoding Ollama model name | Simpler config, fewer moving parts | Cannot test across models, locked to one model's quirks | Only for initial prototype (first week), must be configurable before agent loop works |
| Storing all context in memory (no disk persistence) | Simpler architecture, no file I/O | Agent cannot survive restarts, no cumulative progress, limits session length | Never for a harness designed for infinite loops |
| Logging agent output only to TUI (no file logging) | Fewer files to manage, simpler architecture | Lose all history if TUI crashes, cannot debug past sessions, no audit trail | Never -- file logging is a baseline requirement |
| Using shell `exec` without timeout | Simpler tool call implementation | One hanging command blocks the agent forever | Never -- every shell command needs a timeout |
| Skipping process group management | Simpler process spawning code | Orphan processes accumulate, memory leaks on restarts | Never -- use process groups from the start |
| Single-process architecture (agent + TUI in one process) | Simpler initial implementation, shared state is easy | TUI blocks agent or vice versa, cannot scale, hard to refactor | Only for proof-of-concept demo, must separate before real use |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Ollama API | Assuming default context window is sufficient (4096 tokens) | Always set `num_ctx` explicitly; for agent workloads use 32K-64K minimum |
| Ollama API | Not handling model loading latency | First request after cold start can take 10-30 seconds for model loading. Implement a readiness check or pre-warm the model on startup. |
| Ollama API | Spawning multiple model instances for sub-agents | Use `OLLAMA_NUM_PARALLEL` to handle concurrent requests on a single model instance instead of loading multiple copies |
| Ollama API | Not setting `OLLAMA_MAX_LOADED_MODELS` | Defaults to 3x GPU count, which can exceed available memory. Set explicitly based on hardware. |
| Ollama streaming | Treating streaming responses as complete messages | Parse streaming chunks incrementally. An aborted stream is a partial response -- detect and handle. |
| macOS sandbox (Seatbelt) | Using only filesystem restrictions without network isolation | Seatbelt profiles must restrict both filesystem and network. Without network isolation, a compromised agent exfiltrates via curl/wget. |
| Node.js child processes | Using `child_process.exec` for agent shell commands | Use `child_process.spawn` with explicit timeout, maxBuffer, and signal handling. `exec` buffers all output in memory and can OOM on large outputs. |
| Ink TUI | Rendering every state change immediately | Batch updates and render on a fixed interval (100ms). Debounce log line additions. |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Unbounded shell output capture | Agent runs a command that produces megabytes of output (e.g., `find /`, `cat` large file), filling memory | Limit captured output to a configurable max (e.g., 100KB). Truncate with a "[truncated]" marker. Stream output to file, only load summary into context. | First time agent runs a verbose command |
| KV cache growth with long context | Ollama becomes slow, inference time increases linearly with context length, eventual OOM | Set num_ctx based on available VRAM. An 8B model at 32K context needs approximately 4.5GB KV cache (FP16). Use q8_0 or q4_0 quantization to halve cache size. | When agent sessions exceed 10-15 minutes or context fills past 50% |
| TUI re-render storm | CPU spikes, terminal flickers, agent loop slows down due to event loop contention | Throttle renders to 10fps max. Use `shouldComponentUpdate` or `React.memo` to prevent unnecessary re-renders. | When agent enters a fast tool-call loop (multiple calls per second) |
| Synchronous file I/O in agent loop | Agent hangs during disk writes, especially on HDDs or when writing large state files | Use async I/O for all file operations. Write state files atomically (write to temp, rename). | First time state file exceeds a few KB or disk is under load |
| Ollama model reload on config change | Changing `num_ctx` or other parameters triggers a full model reload (10-30 seconds) | Set all Ollama parameters at initial model load time. Avoid changing configuration mid-session. | First time you try to adjust context mid-run |
| Context compaction overhead | Summarizing context takes an LLM call, which itself costs time and tokens, creating a recursive resource problem | Use heuristic compaction (truncation, structured extraction) before LLM-based summarization. Only use LLM summarization for high-value context. | When compaction frequency exceeds 1 per 5 minutes |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Trusting agent-generated shell commands without validation | Agent can execute arbitrary commands, including destructive ones or sandbox escapes | Maintain a denylist of dangerous patterns (sudo, rm -rf /, chmod 777, curl pipe to sh). Enforce OS-level sandbox as primary defense, denylist as secondary. |
| Allowing the agent to write to its own config files | Agent modifies its own system prompt, sandbox rules, or tool definitions, bypassing all safety measures | Mount config files as read-only. Separate agent workspace from harness configuration directory. |
| Allowing network access by default | Agent can exfiltrate workspace contents, download malicious payloads, or call external APIs | Default to no network access. Allowlist specific endpoints (e.g., Ollama API on localhost) if needed. |
| Running the harness as root or with elevated privileges | Any sandbox escape immediately has full system access | Run the harness as an unprivileged user. Never use sudo in the agent's shell environment. |
| Storing secrets in the agent workspace | Agent can read and exfiltrate API keys, SSH keys, or credentials placed in its workspace | Never store secrets in the workspace. Use environment variables only for the harness process, not passed to the agent's shell. |
| Not auditing agent actions | Cannot detect or investigate malicious or erroneous behavior after the fact | Log every shell command and its output to an append-only audit log outside the agent's workspace. Include timestamps and exit codes. |
| Allowing agent to modify harness source code | Agent modifies its own execution environment to remove safety checks | Mount harness source as read-only. Run harness code from a location outside the agent workspace. |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| No way to interrupt a running agent without killing the process | User must Ctrl+C, potentially losing unsaved state and leaving orphan processes | Implement a "pause" command that gracefully completes the current tool call and then waits. Save state before pausing. |
| Agent activity not visible in TUI until cycle completes | User stares at a blank screen for minutes, unsure if agent is working or stuck | Stream agent activity in real-time: show current tool call, streaming LLM output, and elapsed time. |
| No indication of resource usage | User doesn't know the agent is about to exhaust memory until the system freezes | Show memory usage, token count, and cycle count in the TUI status bar. Color-code warnings. |
| Error messages shown in raw LLM format | User sees inscrutable model output or JSON error dumps | Parse and format errors into human-readable messages. Show the action that failed and a suggested fix. |
| No way to review agent's planned actions before execution | User cannot intervene before a potentially destructive command runs | Implement an optional "approval mode" where dangerous commands (file deletion, package installation) require user confirmation. |
| Terminal state corruption on crash | User's terminal is left in raw mode, alternate screen, or with broken colors after a TUI crash | Register cleanup handlers for all exit paths (SIGINT, SIGTERM, uncaughtException, unhandledRejection). Always restore terminal state. |

## "Looks Done But Isn't" Checklist

- [ ] **Shell sandboxing:** Often missing network isolation -- verify agent cannot make outbound HTTP requests
- [ ] **Shell sandboxing:** Often missing child process inheritance -- verify sandbox rules apply to processes spawned by the agent's commands, not just the agent process itself
- [ ] **Loop detection:** Often only checks exact repetition -- verify it catches near-repetition (same command with trivially different arguments)
- [ ] **Context management:** Often only tracks input tokens -- verify output tokens are also counted against the budget (they consume KV cache too)
- [ ] **Sub-agent cleanup:** Often handles clean shutdown but not crash cleanup -- verify orphan processes are reaped on next harness startup
- [ ] **Ollama integration:** Often tested with one model -- verify context window, memory usage, and latency with the actual target model at the actual target context size
- [ ] **TUI graceful shutdown:** Often restores terminal on Ctrl+C but not on crash -- verify terminal state is restored on uncaught exceptions and SIGTERM
- [ ] **Persistence bootstrap:** Often tested with small state files -- verify the agent can bootstrap from a state file that represents 50+ cycles of accumulated work
- [ ] **Agent timeout:** Often has a per-command timeout but not a per-cycle timeout -- verify the agent cannot spend infinite time on a single exploration cycle
- [ ] **Audit logging:** Often logs commands but not outputs -- verify the full command+output is captured for post-hoc investigation

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Agent stuck in repetitive loop | LOW | Kill the cycle, clear recent context, adjust temperature or prompt, restart. State file preserves prior progress. |
| Agent escapes sandbox and modifies system files | HIGH | Identify all modified files from audit log. Restore from backups. Strengthen sandbox rules. May require system reinstall if root was compromised. |
| Ollama OOM from too many loaded models | LOW | Restart Ollama service. Lower `OLLAMA_MAX_LOADED_MODELS`. Kill orphan model processes. Memory is reclaimed on restart. |
| Context rot causes agent to produce bad findings | MEDIUM | Discard findings from the degraded session (identify via quality metrics or timestamps). Re-run with context compaction enabled. May need to manually review and prune accumulated state. |
| Sub-agent orphan processes consuming memory | LOW | Kill orphan processes (find by PPID=1 and command pattern). Add process group cleanup to harness startup routine to catch leftovers from prior crashes. |
| TUI crash corrupts terminal state | LOW | Run `reset` or `stty sane` to restore terminal. Add terminal restoration to harness startup as a safety measure. |
| Persistence state file corrupted or too large | MEDIUM | If corrupted: restore from last good backup (implement rotating backups of state file). If too large: manually summarize or truncate, or run a one-time compaction script. |
| Cascading sub-agent failure | MEDIUM | Kill all sub-agents. Review audit logs to find the root failure. Fix the root cause. Restart in single-agent mode to verify fix before re-enabling sub-agents. |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Repetitive action loops | Core agent loop | Run agent for 100+ iterations and verify no cycle repeats the same action more than 3 times. Inject a deliberately unsolvable task and verify the agent gives up gracefully. |
| Uncontained shell access | Sandbox/security foundation (Phase 1) | Attempt known escape vectors (write outside workspace, network access, process escalation) from within the sandbox and verify all are blocked. |
| Ollama default context window | Ollama integration | Log actual num_ctx on every API call. Run a session that requires >4K context and verify no silent truncation. |
| Sub-agent resource exhaustion | Sub-agent orchestration | Spawn max concurrent sub-agents on target hardware and verify system remains responsive. Kill the harness mid-run and verify no orphan processes remain. |
| Context rot | Context management | Run a 30+ minute agent session and compare output quality of early vs. late cycles. Verify compaction triggers before the pre-rot threshold. |
| TUI blocking agent | TUI implementation | Measure agent cycle time with and without TUI active. Verify less than 5% performance difference. Simulate 1000 log lines/second and verify TUI doesn't freeze. |
| Bootstrap persistence failure | Core architecture/persistence design | Run 10 consecutive start-stop-start cycles and verify the agent references prior findings in each new session. Grow state file to 50+ cycles and verify bootstrap still works within token budget. |

## Sources

- [Why Do Multi-Agent LLM Systems Fail? (MAST taxonomy, Cemri et al., 2025)](https://arxiv.org/abs/2503.13657) - HIGH confidence, peer-reviewed research with 1600+ annotated traces
- [Practical Security Guidance for Sandboxing Agentic Workflows (NVIDIA, 2025)](https://developer.nvidia.com/blog/practical-security-guidance-for-sandboxing-agentic-workflows-and-managing-execution-risk/) - HIGH confidence, vendor guidance with specific technical recommendations
- [Claude Code Sandboxing (Anthropic, 2025)](https://www.anthropic.com/engineering/claude-code-sandboxing) - HIGH confidence, production-tested approach combining Seatbelt and Bubblewrap
- [The Context Window Problem: Scaling Agents Beyond Token Limits (Factory.ai)](https://factory.ai/news/context-window-problem) - MEDIUM confidence, practitioner insights
- [Context Rot research (Chroma/Hong et al., 2025)](https://www.getmaxim.ai/articles/context-window-management-strategies-for-long-context-ai-agents-and-chatbots/) - MEDIUM confidence, measurements across 18 LLMs
- [Ollama Context Length Documentation](https://docs.ollama.com/context-length) - HIGH confidence, official documentation
- [Ollama Memory Management (DeepWiki)](https://deepwiki.com/ollama/ollama/5.4-memory-management-and-gpu-allocation) - MEDIUM confidence, technical analysis of Ollama internals
- [How Ollama Handles Parallel Requests (Glukhov, 2025)](https://www.glukhov.org/post/2025/05/how-ollama-handles-parallel-requests/) - MEDIUM confidence, practitioner analysis
- [Orphan AI agent process leak (Gastown issue #29, 2026)](https://github.com/steveyegge/gastown/issues/29) - HIGH confidence, documented real-world issue
- [From ReAct to Ralph Loop (Alibaba, 2025)](https://www.alibabacloud.com/blog/from-react-to-ralph-loop-a-continuous-iteration-paradigm-for-ai-agents_602799) - MEDIUM confidence, production-informed architectural pattern
- [Why Your Multi-Agent System is Failing (Towards Data Science, 2025)](https://towardsdatascience.com/why-your-multi-agent-system-is-failing-escaping-the-17x-error-trap-of-the-bag-of-agents/) - MEDIUM confidence, practitioner analysis with quantitative findings
- [AI Agents Deleting Home Folders? Run Your Agent in Firejail (SES, 2025)](https://softwareengineeringstandard.com/2025/12/15/ai-agents-firejail-sandbox/) - MEDIUM confidence, practical guide with real failure scenario
- [LLM Agent Loop Stuck (SuperAGI issue #542)](https://github.com/TransformerOptimus/SuperAGI/issues/542) - HIGH confidence, documented real-world issue
- [Reliability for Unreliable LLMs (Stack Overflow, 2025)](https://stackoverflow.blog/2025/06/30/reliability-for-unreliable-llms/) - MEDIUM confidence, practitioner guidance
- [Snyk: From SKILL.md to Shell Access in Three Lines of Markdown](https://snyk.io/articles/skill-md-shell-access/) - HIGH confidence, security vendor with specific attack demonstration

---
*Pitfalls research for: Autonomous AI agent harness (Ouroboros/ouro)*
*Researched: 2026-02-03*
