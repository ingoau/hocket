// The typed server address as the core should get it: trimmed, without
// trailing slashes, and with the scheme lower-cased (people and keyboards
// capitalise it, as in "Https://"). Undefined when it doesn't start with
// http:// or https://, in any case, followed by a host.
export function normaliseServerUrl(raw: string): string | undefined {
  const [, scheme, rest] = /^(https?):\/\/(.+)$/i.exec(raw.trim().replace(/\/+$/, "")) ?? [];
  return scheme && rest ? `${scheme.toLowerCase()}://${rest}` : undefined;
}
