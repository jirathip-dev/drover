// Generated from crates/server/src/serve/api.rs by crates/server/gen/api_ts.rs: do not edit.

export type State = { seq: number, age: number | null, mtime: string, theme: string, fleets: Array<Fleet>, queue: Array<Queued>, lanes: Array<Lane>, asks: Array<Ask>, hosts: Array<Row>, services: Array<Row>, alerts: Array<Row>, };

export type Fleet = { name: string, color: string, paused: boolean, };

export type Queued = { fleet: string, issue: number, title: string | null, state: string, blocked_by: number | null, };

export type Lane = { name: string, fleet: string | null, issue: number | null, title: string | null, role: string, harness: string, model: string | null, state: string, shown: string, tone: string, since: number, };

export type Ask = { id: string, fleet: string, issue: number | null, question: string, options: Array<string>, recommend: number | null, ts: number, };

export type Row = { name: string, tone: string, detail: string, fix: string, };

export type Events = { seq: number, refresh: boolean, list: Array<Ev>, };

export type Ev = { seq: number, ts: number, fleet: string, issue: number | null, type: string, data: { [key in string]: JsonValue }, k: string | null, };

export type Reply = { ok: boolean, code: string, message: string, };

export type Transcript = { harness: string, lines: Array<string>, live: number, before: number, more: boolean, acts: Array<string>, pane: string | null, status: string | null, said: Array<JsonValue> | null, notices: Array<Notice>, };

export type Notice = { at: number, text: string, };

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
