// `hocket://` deep links. Reserved and routed; the renderer interprets the
// path (`hocket://album/<id>`, `hocket://track/<id>`, `hocket://open`).
import { app } from "electron";
import { DEEP_LINK_SCHEME } from "@shared/constants";

export function registerDeepLinks(isDev: boolean, appRoot: string): void {
  if (process.defaultApp || isDev) {
    // Development: the executable is `electron`, so pass the entry as an argument.
    if (process.argv[1]) app.setAsDefaultProtocolClient(DEEP_LINK_SCHEME, process.execPath, [appRoot]);
  } else {
    app.setAsDefaultProtocolClient(DEEP_LINK_SCHEME);
  }
}

/** Extract a hocket:// URL from a process argv list (Windows/Linux second-instance). */
export function deepLinkFromArgv(argv: string[]): string | undefined {
  return argv.find((a) => a.startsWith(`${DEEP_LINK_SCHEME}://`));
}
