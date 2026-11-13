// The settings a setup link carries when there is no coordinator to sync them
// (Set up another device): `Query.setupSettings`'s JSON object, zlib-deflated
// and in unpadded base64url so the code stays small and the value link-safe.
// Android packs and unpacks the same way (SetupLink.packSettings/unpackSettings).

/** Unpacked settings past this size are refused (a link is a few KiB at most). */
const MAX_UNPACKED = 64 * 1024;

export async function packSetupSettings(json: string): Promise<string> {
  const stream = new Blob([json]).stream().pipeThrough(new CompressionStream("deflate"));
  const bytes = new Uint8Array(await new Response(stream).arrayBuffer());
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** [packSetupSettings] undone, or undefined when [packed] is not a packed JSON object. */
export async function unpackSetupSettings(packed: string): Promise<string | undefined> {
  try {
    const b64 = packed.replace(/-/g, "+").replace(/_/g, "/");
    const binary = atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4));
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    const reader = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("deflate")).getReader();
    const decoder = new TextDecoder();
    let json = "";
    let size = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > MAX_UNPACKED) {
        await reader.cancel();
        return undefined;
      }
      json += decoder.decode(value, { stream: true });
    }
    json += decoder.decode();
    const parsed: unknown = JSON.parse(json);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? json : undefined;
  } catch {
    return undefined;
  }
}

/** How many settings a `Query.setupSettings` answer holds (0 when it isn't one). */
export function setupSettingsCount(json: string | undefined): number {
  try {
    const parsed: unknown = json ? JSON.parse(json) : undefined;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? Object.keys(parsed).length : 0;
  } catch {
    return 0;
  }
}
