// Artwork tokens for the hocket-art:// protocol. Main answers Query.Artwork
// with an opaque token instead of the path so the renderer can never name a
// file; a token resolves only to a file that lives (after realpath) inside
// the image cache directory.
import { randomBytes } from "node:crypto";
import { realpath } from "node:fs/promises";
import { resolve, sep } from "node:path";
import { ART_TOKEN_RE, artworkFilePath } from "@shared/constants";

function within(file: string, root: string): boolean {
  return file === root || file.startsWith(root.endsWith(sep) ? root : root + sep);
}

export class ArtworkRegistry {
  private readonly byPath = new Map<string, string>();
  private readonly byToken = new Map<string, string>();

  /** `root` is the image cache directory (already realpath'd by the caller). */
  constructor(
    private readonly root: string,
    private readonly limit = 20_000,
  ) {}

  /** Returns a token for `pathOrUrl`, or undefined when it doesn't resolve to a file under the cache. */
  async register(pathOrUrl: string): Promise<string | undefined> {
    const real = await this.realFile(pathOrUrl);
    if (!real) return undefined;
    const existing = this.byPath.get(real);
    if (existing) return existing;
    const token = randomBytes(16).toString("hex");
    this.byPath.set(real, token);
    this.byToken.set(token, real);
    if (this.byToken.size > this.limit) {
      const oldest = this.byToken.keys().next().value as string;
      const path = this.byToken.get(oldest);
      this.byToken.delete(oldest);
      if (path !== undefined) this.byPath.delete(path);
    }
    return token;
  }

  /** The file behind a token, re-checked against the cache root at serve time (a symlink may have been swapped in since). */
  async resolve(token: string): Promise<string | undefined> {
    if (typeof token !== "string" || !ART_TOKEN_RE.test(token)) return undefined;
    const path = this.byToken.get(token);
    if (!path) return undefined;
    return this.realFile(path);
  }

  private async realFile(pathOrUrl: string): Promise<string | undefined> {
    try {
      const real = await realpath(resolve(artworkFilePath(pathOrUrl)));
      return within(real, this.root) ? real : undefined;
    } catch {
      return undefined;
    }
  }
}
