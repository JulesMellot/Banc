// Client of the server's HTTP API (crates/banc-server/src/api.rs).
// The session cookie is HttpOnly: the browser sends it, this code never sees it.

export type Os = 'macos' | 'windows' | 'linux';
export type MachineRole = 'workstation' | 'depot' | 'render';
export type PersonRole = 'admin' | 'editor' | 'colorist';
export type InviteStatus = 'pending' | 'used' | 'expired' | 'revoked' | 'obsolete';

export interface Info {
  version: string;
  endpoint_id: string;
  relay_url: string | null;
  setup_required: boolean;
}

export interface Admin {
  id: number;
  name: string;
}

export interface AdminKey {
  public_key: string | null;
  fingerprint: string | null;
}

export interface Machine {
  id: number;
  name: string;
  public_key: string;
  os: Os;
  arch: string;
  role: MachineRole;
  user_name: string;
  person_role: PersonRole;
  enrolled_at: number;
  last_seen_at: number | null;
  agent_version: string;
  os_version: string | null;
  hostname: string | null;
  resolve: { edition: 'studio' | 'free'; version: string } | null;
  revoked_at: number | null;
  online: boolean;
}

export interface InviteRow {
  id: number;
  user_name: string;
  person_role: PersonRole;
  machine_role: MachineRole;
  created_at: number;
  expires_at: number;
  used_at: number | null;
  revoked_at: number | null;
  machine_id: number | null;
  status: InviteStatus;
}

export interface InviteCreated {
  invite: InviteRow;
  link: string;
  key: string;
  server: string;
}

export interface NewInvite {
  user_name: string;
  person_role: PersonRole;
  machine_role: MachineRole;
}

/** An error answered by the server: `code` is stable, `message` is English. */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

type Fetch = typeof fetch;

/** Builds the client. `fetcher` is replaced in tests. */
export function client(fetcher: Fetch = (input, init) => fetch(input, init)) {
  async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const init: RequestInit = { method, credentials: 'same-origin' };
    if (body !== undefined) {
      init.headers = { 'Content-Type': 'application/json' };
      init.body = JSON.stringify(body);
    }
    let response: Response;
    try {
      response = await fetcher(path, init);
    } catch {
      throw new ApiError(0, 'network', 'server unreachable');
    }
    if (response.status === 204) {
      return undefined as T;
    }
    const data: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      const error = (data ?? {}) as { error?: string; message?: string };
      throw new ApiError(
        response.status,
        error.error ?? 'unknown',
        error.message ?? response.statusText,
      );
    }
    return data as T;
  }

  return {
    info: () => request<Info>('GET', '/api/v1/info'),
    setup: (token: string, name: string, password: string) =>
      request<Admin>('POST', '/api/v1/admin/setup', { token, name, password }),
    login: (name: string, password: string) =>
      request<Admin>('POST', '/api/v1/admin/login', { name, password }),
    logout: () => request<void>('POST', '/api/v1/admin/logout'),
    me: () => request<Admin>('GET', '/api/v1/admin/me'),
    adminKey: () => request<AdminKey>('GET', '/api/v1/admin/admin-key'),
    machines: () => request<Machine[]>('GET', '/api/v1/admin/machines'),
    revokeMachine: (id: number) => request<void>('POST', `/api/v1/admin/machines/${id}/revoke`),
    invites: () => request<InviteRow[]>('GET', '/api/v1/admin/invites'),
    createInvite: (invite: NewInvite) =>
      request<InviteCreated>('POST', '/api/v1/admin/invites', invite),
    revokeInvite: (id: number) => request<void>('POST', `/api/v1/admin/invites/${id}/revoke`),
  };
}

export type Api = ReturnType<typeof client>;

export const api = client();
