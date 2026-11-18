// The window's commands (ui/src-tauri/src), typed.
// They return the agent's state or throw { code, message }.
import { invoke } from '@tauri-apps/api/core';

import type { MachineRole, PersonRole } from './api';

export type ConnectionState = 'connecting' | 'connected' | 'revoked';

export interface Inventory {
  hostname: string;
  os_version: string;
  resolve: { edition: 'studio' | 'free' | null; version: string } | null;
}

export interface Enrollment {
  server: string;
  machine_id: number;
  name: string;
  machine_role: MachineRole;
  person_role: PersonRole;
  admin_fingerprint: string;
  connection: ConnectionState;
}

export interface AgentStatus {
  version: string;
  enrollment: Enrollment | null;
  inventory: Inventory;
}

export interface LinkSummary {
  server: string;
  fingerprint: string;
}

export interface KeyInfo {
  fingerprint: string;
}

export interface NewKey {
  fingerprint: string;
  recovery_code: string;
}

/** Error of a command: `code` maps to `error.<code>` in fr.json. */
export class CommandError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** Builds the client. `call` is replaced in tests. */
export function commands(call: Invoke = invoke) {
  async function run<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await call<T>(command, args);
    } catch (error) {
      const failure = (error ?? {}) as { code?: string; message?: string };
      throw new CommandError(failure.code ?? 'unknown', failure.message ?? String(error));
    }
  }

  return {
    status: () => run<AgentStatus>('agent_status'),
    enrollWithLink: (link: string, name: string) =>
      run<AgentStatus>('enroll', { link, name, server: null, key: null }),
    enrollWithKey: (server: string, key: string, name: string) =>
      run<AgentStatus>('enroll', { link: null, server, key, name }),
    installAgent: () => run<void>('install_agent'),
    hostname: () => run<string>('hostname'),
    readLink: (link: string) => run<LinkSummary>('read_link', { link }),
    pendingLink: () => run<string | null>('pending_link'),
    adminKeyStatus: () => run<KeyInfo | null>('admin_key_status'),
    adminKeyGenerate: () => run<NewKey>('admin_key_generate'),
    adminKeyConfirm: (lastGroup: string) => run<KeyInfo>('admin_key_confirm', { lastGroup }),
    adminKeyRestore: (code: string) => run<KeyInfo>('admin_key_restore', { code }),
    adminKeyRegister: (server: string, name: string, password: string, replace: boolean) =>
      run<KeyInfo>('admin_key_register', { server, name, password, replace }),
  };
}

export type Commands = ReturnType<typeof commands>;

/** Whether the page runs inside the Banc window rather than a browser. */
export function inWindow(scope: object = globalThis): boolean {
  return '__TAURI_INTERNALS__' in scope;
}
