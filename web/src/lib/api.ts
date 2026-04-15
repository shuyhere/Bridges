const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "";

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const token =
    typeof window !== "undefined"
      ? localStorage.getItem("bridges_session")
      : null;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(options.headers as Record<string, string>),
  };

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers,
  });

  if (!res.ok) {
    const body = await res.json().catch(() => ({ message: res.statusText }));
    throw new ApiError(res.status, body.message || res.statusText);
  }

  return res.json();
}

// ── Auth ──

export interface AuthResp {
  sessionToken: string;
  user: User;
}

export interface User {
  userId: string;
  email: string;
  displayName: string | null;
  emailVerified: boolean;
  plan: string;
  createdAt: string;
}

export function signup(
  email: string,
  password: string,
  displayName?: string,
): Promise<AuthResp> {
  return request("/v1/auth/signup", {
    method: "POST",
    body: JSON.stringify({ email, password, displayName }),
  });
}

export function login(email: string, password: string): Promise<AuthResp> {
  return request("/v1/auth/login", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  });
}

export function getMe(): Promise<User> {
  return request("/v1/user/me");
}

export function updateProfile(data: {
  displayName?: string;
  email?: string;
}): Promise<{ ok: boolean; message: string }> {
  return request("/v1/user/me", {
    method: "PATCH",
    body: JSON.stringify(data),
  });
}

export function changePassword(
  currentPassword: string,
  newPassword: string,
): Promise<{ ok: boolean; message: string }> {
  return request("/v1/user/change-password", {
    method: "POST",
    body: JSON.stringify({ currentPassword, newPassword }),
  });
}

// ── Tokens ──

export interface Token {
  tokenId: string;
  name: string;
  scopes: string;
  prefix: string;
  lastUsedAt: string | null;
  expiresAt: string | null;
  createdAt: string;
}

export interface CreateTokenResp {
  tokenId: string;
  token: string;
  name: string;
  scopes: string;
  expiresAt: string | null;
  createdAt: string;
}

export function listTokens(): Promise<Token[]> {
  return request("/v1/tokens");
}

export function createToken(
  name: string,
  expiresIn?: number,
): Promise<CreateTokenResp> {
  return request("/v1/tokens", {
    method: "POST",
    body: JSON.stringify({ name, expiresIn }),
  });
}

export function deleteToken(
  tokenId: string,
): Promise<{ ok: boolean; message: string }> {
  return request(`/v1/tokens/${tokenId}`, { method: "DELETE" });
}

// ── Projects ──

export interface Project {
  projectId: string;
  slug: string;
  displayName: string | null;
  description: string | null;
  createdBy: string;
  giteaOwner: string | null;
  giteaRepo: string | null;
  createdAt: string;
}

export function listProjects(): Promise<Project[]> {
  return request("/v1/user/projects");
}

export interface Member {
  nodeId: string;
  agentRole: string | null;
  displayName: string | null;
  ed25519Pubkey: string | null;
  joinedAt: string;
}

export function listMembers(projectId: string): Promise<Member[]> {
  return request(`/v1/projects/${projectId}/members`);
}

// ── Invites ──

export interface Invite {
  inviteId: string;
  inviteToken: string;
  projectId: string;
}

export function createInvite(projectId: string): Promise<Invite> {
  return request(`/v1/projects/${projectId}/invites`, {
    method: "POST",
    body: JSON.stringify({ maxUses: 10 }),
  });
}

// ── Contacts ──

export interface Contact {
  nodeId: string;
  displayName: string | null;
  registeredName: string | null;
  addedAt: string;
}

export function listContacts(): Promise<Contact[]> {
  return request("/v1/contacts");
}

export function addContact(nodeId: string, displayName?: string): Promise<{ ok: boolean; message: string }> {
  return request("/v1/contacts", {
    method: "POST",
    body: JSON.stringify({ nodeId, displayName }),
  });
}

export function removeContact(nodeId: string): Promise<{ ok: boolean; message: string }> {
  return request(`/v1/contacts/${nodeId}`, { method: "DELETE" });
}

// ── Session helpers ──

export function saveSession(token: string) {
  localStorage.setItem("bridges_session", token);
}

export function clearSession() {
  localStorage.removeItem("bridges_session");
}

export function hasSession(): boolean {
  return typeof window !== "undefined" && !!localStorage.getItem("bridges_session");
}
