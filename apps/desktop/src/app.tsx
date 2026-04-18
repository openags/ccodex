import { useEffect, useState } from "react";

import {
  forkSession,
  getSession,
  getTurns,
  listExtensions,
  listPendingInteractions,
  listSessions,
  ping,
  resolveApproval,
  resolveAskUser,
  resumePrompt,
  runPrompt,
  subscribeEvents,
  type ApprovalRequest,
  type AskUserPrompt,
  type ExtensionManifest,
  type SessionId,
  type SessionSummary,
  type StoredTurn,
} from "./rpc/client";

function renderTurns(turns: StoredTurn[]): string[] {
  const lines: string[] = [];
  for (const stored of turns) {
    lines.push(`turn ${stored.turn.id}`);
    for (const item of stored.items) {
      lines.push(`  ${JSON.stringify(item.payload)}`);
    }
  }
  return lines;
}

export function App() {
  const [status, setStatus] = useState("Connecting to local-server...");
  const [prompt, setPrompt] = useState("");
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [extensions, setExtensions] = useState<ExtensionManifest[]>([]);
  const [approvals, setApprovals] = useState<ApprovalRequest[]>([]);
  const [askUser, setAskUser] = useState<AskUserPrompt[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<SessionId | null>(null);
  const [planLines, setPlanLines] = useState<string[]>(["no active plan"]);
  const [transcript, setTranscript] = useState<string[]>([
    "ccodex desktop shell",
    "This thin shell connects to apps/local-server over WebSocket.",
  ]);

  async function refreshSidebar() {
    const [nextSessions, nextExtensions, nextPending] = await Promise.all([
      listSessions(),
      listExtensions(),
      listPendingInteractions(),
    ]);
    setSessions(nextSessions);
    setExtensions(nextExtensions);
    setApprovals(nextPending.approvals);
    setAskUser(nextPending.askUser);
    if (!currentSessionId && nextSessions[0]) {
      setCurrentSessionId(nextSessions[0].id);
    }
  }

  async function refreshTranscript(sessionId: SessionId) {
    const turns = await getTurns(sessionId);
    setTranscript(
      turns.length === 0
        ? ["No transcript yet for this session."]
        : renderTurns(turns),
    );
  }

  async function refreshPlan(sessionId: SessionId) {
    const session = await getSession(sessionId);
    if (session.active_plan) {
      setPlanLines([
        session.active_plan.summary,
        ...session.active_plan.items.map(
          (item) => `[${item.status}] ${item.title}`,
        ),
      ]);
    } else {
      setPlanLines(["no active plan"]);
    }
  }

  async function run(promptText: string) {
    const result = currentSessionId
      ? await resumePrompt(currentSessionId, promptText)
      : await runPrompt(promptText);
    setCurrentSessionId(result.session.id);
    setStatus(`ok: session ${result.session.id}`);
    await refreshSidebar();
    await refreshPlan(result.session.id);
    await refreshTranscript(result.session.id);
  }

  useEffect(() => {
    let mounted = true;
    ping()
      .then(async (serverStatus) => {
        if (!mounted) {
          return;
        }
        setStatus(`connected: ${serverStatus}`);
        await refreshSidebar();
        if (currentSessionId) {
          await refreshPlan(currentSessionId);
        }
      })
      .catch((error) => {
        if (!mounted) {
          return;
        }
        setStatus(`server unavailable: ${String(error)}`);
      });

    const unsubscribe = subscribeEvents(async () => {
      try {
        await refreshSidebar();
        if (currentSessionId) {
          await refreshPlan(currentSessionId);
          await refreshTranscript(currentSessionId);
        }
      } catch {
        // Ignore refresh races while the shell stays mounted.
      }
    });

    return () => {
      mounted = false;
      unsubscribe();
    };
  }, [currentSessionId]);

  return (
    <div className="shell">
      <header className="topbar">
        <div>
          <h1>ccodex desktop</h1>
          <p>{status}</p>
        </div>
      </header>

      <main className="layout">
        <aside className="sidebar">
          <section className="panel">
            <h2>Sessions</h2>
            <ul>
              {sessions.map((session) => (
                <li key={session.id}>
                  <button
                    className={session.id === currentSessionId ? "active" : ""}
                    onClick={async () => {
                      setCurrentSessionId(session.id);
                      await refreshPlan(session.id);
                      await refreshTranscript(session.id);
                    }}
                  >
                    <span>{session.title || "Untitled"}</span>
                    <small>{session.id}</small>
                  </button>
                  <button
                    type="button"
                    className="secondary"
                    onClick={async () => {
                      const forked = await forkSession(session.id);
                      setCurrentSessionId(forked.id);
                      setStatus(`forked: ${forked.id}`);
                      await refreshSidebar();
                      await refreshPlan(forked.id);
                      await refreshTranscript(forked.id);
                    }}
                  >
                    Fork
                  </button>
                </li>
              ))}
            </ul>
          </section>

          <section className="panel">
            <h2>Extensions</h2>
            <ul>
              {extensions.map((manifest) => (
                <li key={`${manifest.kind}-${manifest.name}`}>
                  <strong>{manifest.kind}</strong>
                  <span>{manifest.name}</span>
                </li>
              ))}
            </ul>
          </section>

          <section className="panel">
            <h2>Approvals</h2>
            <ul>
              {approvals.map((request) => (
                <li key={request.item_id}>
                  <div>{request.summary}</div>
                  <small>{request.command || request.risk}</small>
                  <div className="button-row">
                    <button onClick={() => resolveApproval(request.item_id, "Approved")}>
                      Approve
                    </button>
                    <button onClick={() => resolveApproval(request.item_id, "Rejected")}>
                      Reject
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          </section>

          <section className="panel">
            <h2>Ask User</h2>
            <ul>
              {askUser.map((request) => (
                <li key={request.item_id}>
                  <div>{request.title}</div>
                  <div className="button-row wrap">
                    {request.choices.map((choice) => (
                      <button
                        key={choice.id}
                        onClick={() =>
                          resolveAskUser(request.item_id, {
                            selectedChoiceId: choice.id,
                          })
                        }
                      >
                        {choice.label}
                      </button>
                    ))}
                  </div>
                </li>
              ))}
            </ul>
          </section>

          <section className="panel">
            <h2>Plan</h2>
            <ul>
              {planLines.map((line, index) => (
                <li key={`${line}-${index}`}>{line}</li>
              ))}
            </ul>
          </section>
        </aside>

        <section className="workspace">
          <div className="panel transcript">
            <h2>Transcript</h2>
            <pre>{transcript.join("\n")}</pre>
          </div>

          <form
            className="composer"
            onSubmit={async (event) => {
              event.preventDefault();
              const nextPrompt = prompt.trim();
              if (!nextPrompt) {
                return;
              }
              setPrompt("");
              await run(nextPrompt);
            }}
          >
            <textarea
              value={prompt}
              onChange={(event) => setPrompt(event.target.value)}
              placeholder="Send a prompt through local-server..."
            />
            <button type="submit">Run</button>
          </form>
        </section>
      </main>
    </div>
  );
}
