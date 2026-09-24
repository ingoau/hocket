// Config backup secrets. The core only ever writes opaque references
// (`keystore://hocket/server/<id>/password`, see settings/config.rs) into
// `ConfigDocument.secrets`; the platform resolves them from its credential
// store on export and turns plain-text entries back into AddServer on import.
// Pure functions: the IPC layer owns the dialogs and the dispatching.
import type { ConfigDocument, ServerInfo } from "@core/api";

const KEYSTORE_REF_RE = /^keystore:\/\/hocket\/server\/([^/]+)\/password$/;
const SECRET_KEY_RE = /^server:(.+):password$/;

export interface ServerSecret {
  url: string;
  username: string;
  name: string | undefined;
  password: string;
}

export function isKeystoreReference(value: unknown): boolean {
  return typeof value === "string" && KEYSTORE_REF_RE.test(value);
}

/** Parse a document leniently: only the shape main relies on is checked. */
export function parseConfigDocument(text: string): ConfigDocument {
  const doc = JSON.parse(text) as unknown;
  if (!doc || typeof doc !== "object" || Array.isArray(doc)) throw new Error("not a config document");
  const d = doc as ConfigDocument;
  if (d.secrets !== undefined && (typeof d.secrets !== "object" || d.secrets === null || Array.isArray(d.secrets))) throw new Error("bad secrets map");
  if (d.servers !== undefined && !Array.isArray(d.servers)) throw new Error("bad servers list");
  return d;
}

function serverById(doc: ConfigDocument, id: string): ServerInfo | undefined {
  return (doc.servers ?? []).find((s) => s && typeof s === "object" && s.id === id);
}

/**
 * Replace every keystore reference with the password `lookup` knows for that
 * server. References nobody can resolve are dropped; an empty map is removed.
 */
export function resolveSecretReferences(doc: ConfigDocument, lookup: (url: string, username: string) => string | undefined): { document: ConfigDocument; resolved: number; unresolved: number } {
  if (!doc.secrets) return { document: doc, resolved: 0, unresolved: 0 };
  const out: Record<string, string> = {};
  let resolved = 0;
  let unresolved = 0;
  for (const [key, value] of Object.entries(doc.secrets)) {
    const ref = typeof value === "string" ? KEYSTORE_REF_RE.exec(value) : null;
    if (!ref) {
      // Already plain text (or unknown): pass through untouched.
      if (typeof value === "string") out[key] = value;
      continue;
    }
    const server = serverById(doc, ref[1] as string);
    const password = server ? lookup(server.url, server.username) : undefined;
    if (password === undefined) {
      unresolved++;
      continue;
    }
    out[key] = password;
    resolved++;
  }
  const document: ConfigDocument = { ...doc, secrets: Object.keys(out).length ? out : undefined };
  if (!document.secrets) delete document.secrets;
  return { document, resolved, unresolved };
}

/**
 * Pull plain-text passwords out of an imported document. Returns the servers
 * to add (only those the document also describes) and the document with the
 * secrets stripped, which is what goes to the core.
 */
export function extractSecrets(doc: ConfigDocument): { document: ConfigDocument; servers: ServerSecret[] } {
  const servers: ServerSecret[] = [];
  for (const [key, value] of Object.entries(doc.secrets ?? {})) {
    const m = SECRET_KEY_RE.exec(key);
    if (!m || typeof value !== "string" || !value || isKeystoreReference(value)) continue;
    const server = serverById(doc, m[1] as string);
    if (!server || typeof server.url !== "string" || typeof server.username !== "string") continue;
    servers.push({ url: server.url, username: server.username, name: typeof server.name === "string" && server.name ? server.name : undefined, password: value });
  }
  const document = { ...doc };
  delete document.secrets;
  return { document, servers };
}
