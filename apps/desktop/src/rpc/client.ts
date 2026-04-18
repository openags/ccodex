export type SessionId = string;
export type ItemId = string;

export interface SessionSummary {
  id: SessionId;
  title?: string | null;
  updated_at: string;
  active_plan?: PlanState | null;
}

export interface PlanItem {
  id: string;
  title: string;
  status: string;
  notes?: string | null;
}

export interface PlanState {
  id: string;
  summary: string;
  items: PlanItem[];
}

export interface ExtensionManifest {
  kind: string;
  name: string;
  description?: string | null;
  source_path: string;
}

export interface ApprovalRequest {
  item_id: ItemId;
  summary: string;
  command?: string | null;
  risk: string;
}

export interface AskUserChoice {
  id: string;
  label: string;
  description?: string | null;
}

export interface AskUserPrompt {
  item_id: ItemId;
  title: string;
  message: string;
  choices: AskUserChoice[];
  allow_freeform: boolean;
}

export interface StoredItem {
  id: ItemId;
  payload: Record<string, unknown>;
}

export interface StoredTurn {
  turn: {
    id: string;
    session_id: SessionId;
  };
  items: StoredItem[];
}

export interface TurnResult {
  session: SessionSummary;
  turn: {
    id: string;
  };
  assistant_text: string;
}

type RpcRequestBody =
  | { Ping: null }
  | { RunPrompt: { prompt: string } }
  | { ResumePrompt: { session_id: SessionId; prompt: string } }
  | { ForkSession: { session_id: SessionId } }
  | { ListSessions: { limit?: number | null } }
  | { GetSession: { session_id: SessionId } }
  | { GetTurns: { session_id: SessionId } }
  | { ListExtensions: null }
  | { ListPendingInteractions: null }
  | { ResolveApproval: { response: { request_item_id: ItemId; decision: string; reason?: string | null } } }
  | { ResolveAskUser: { response: { request_item_id: ItemId; selected_choice_id?: string | null; freeform_text?: string | null } } };

type RpcResponseBody =
  | { Pong: { product: string; version: string; protocol: string } }
  | { TurnResult: TurnResult }
  | { Sessions: { sessions: SessionSummary[] } }
  | { Session: { session: SessionSummary } }
  | { Turns: { turns: StoredTurn[] } }
  | { Extensions: { manifests: ExtensionManifest[] } }
  | { PendingInteractions: { approvals: ApprovalRequest[]; ask_user: AskUserPrompt[] } }
  | { Event: { event: unknown } }
  | { InteractionResolved: { request_item_id: ItemId } }
  | { Subscribed: null }
  | { Error: { code: string; message: string } };

interface RpcEnvelope {
  id: string;
  body: RpcResponseBody;
}

const DEFAULT_LOCAL_SERVER_ADDR = "127.0.0.1:48765";

function localServerAddress(): string {
  return (
    (globalThis as { __CCODEX_LOCAL_SERVER_ADDR__?: string }).__CCODEX_LOCAL_SERVER_ADDR__ ??
    DEFAULT_LOCAL_SERVER_ADDR
  );
}

function localServerUrl(): string {
  return `ws://${localServerAddress()}`;
}

function requestId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

async function sendRequest<T extends RpcResponseBody>(body: RpcRequestBody): Promise<T> {
  const socket = new WebSocket(localServerUrl());
  const request = {
    id: requestId("desktop"),
    body,
  };

  const response = await new Promise<RpcEnvelope>((resolve, reject) => {
    socket.onerror = () => reject(new Error("desktop websocket request failed"));
    socket.onopen = () => {
      socket.send(JSON.stringify(request));
    };
    socket.onmessage = (event) => {
      try {
        resolve(JSON.parse(event.data as string) as RpcEnvelope);
      } catch (error) {
        reject(error);
      } finally {
        socket.close();
      }
    };
  });

  if ("Error" in response.body) {
    throw new Error(`${response.body.Error.code}: ${response.body.Error.message}`);
  }
  return response.body as T;
}

export async function ping(): Promise<string> {
  const response = await sendRequest<{ Pong: { product: string; version: string; protocol: string } }>({
    Ping: null,
  });
  return `${response.Pong.product} ${response.Pong.version} (${response.Pong.protocol})`;
}

export async function runPrompt(prompt: string): Promise<TurnResult> {
  const response = await sendRequest<{ TurnResult: TurnResult }>({
    RunPrompt: { prompt },
  });
  return response.TurnResult;
}

export async function resumePrompt(sessionId: SessionId, prompt: string): Promise<TurnResult> {
  const response = await sendRequest<{ TurnResult: TurnResult }>({
    ResumePrompt: { session_id: sessionId, prompt },
  });
  return response.TurnResult;
}

export async function forkSession(sessionId: SessionId): Promise<SessionSummary> {
  const response = await sendRequest<{ Session: { session: SessionSummary } }>({
    ForkSession: { session_id: sessionId },
  });
  return response.Session.session;
}

export async function listSessions(limit = 12): Promise<SessionSummary[]> {
  const response = await sendRequest<{ Sessions: { sessions: SessionSummary[] } }>({
    ListSessions: { limit },
  });
  return response.Sessions.sessions;
}

export async function getTurns(sessionId: SessionId): Promise<StoredTurn[]> {
  const response = await sendRequest<{ Turns: { turns: StoredTurn[] } }>({
    GetTurns: { session_id: sessionId },
  });
  return response.Turns.turns;
}

export async function getSession(sessionId: SessionId): Promise<SessionSummary> {
  const response = await sendRequest<{ Session: { session: SessionSummary } }>({
    GetSession: { session_id: sessionId },
  });
  return response.Session.session;
}

export async function listExtensions(): Promise<ExtensionManifest[]> {
  const response = await sendRequest<{ Extensions: { manifests: ExtensionManifest[] } }>({
    ListExtensions: null,
  });
  return response.Extensions.manifests;
}

export async function listPendingInteractions(): Promise<{
  approvals: ApprovalRequest[];
  askUser: AskUserPrompt[];
}> {
  const response = await sendRequest<{
    PendingInteractions: { approvals: ApprovalRequest[]; ask_user: AskUserPrompt[] };
  }>({
    ListPendingInteractions: null,
  });
  return {
    approvals: response.PendingInteractions.approvals,
    askUser: response.PendingInteractions.ask_user,
  };
}

export async function resolveApproval(
  requestItemId: ItemId,
  decision: "Approved" | "Rejected" | "Cancelled",
  reason?: string,
): Promise<void> {
  await sendRequest<{ InteractionResolved: { request_item_id: ItemId } }>({
    ResolveApproval: {
      response: {
        request_item_id: requestItemId,
        decision,
        reason,
      },
    },
  });
}

export async function resolveAskUser(
  requestItemId: ItemId,
  payload: { selectedChoiceId?: string; freeformText?: string },
): Promise<void> {
  await sendRequest<{ InteractionResolved: { request_item_id: ItemId } }>({
    ResolveAskUser: {
      response: {
        request_item_id: requestItemId,
        selected_choice_id: payload.selectedChoiceId ?? null,
        freeform_text: payload.freeformText ?? null,
      },
    },
  });
}

export function subscribeEvents(onEvent: (event: unknown) => void): () => void {
  const socket = new WebSocket(localServerUrl());
  socket.onopen = () => {
    socket.send(
      JSON.stringify({
        id: requestId("desktop-sub"),
        body: { SubscribeEvents: null },
      }),
    );
  };
  socket.onmessage = (event) => {
    try {
      const payload = JSON.parse(event.data as string) as RpcEnvelope;
      if ("Event" in payload.body) {
        onEvent(payload.body.Event.event);
      }
    } catch {
      // Ignore malformed event frames to keep the shell resilient.
    }
  };

  return () => {
    socket.close();
  };
}
