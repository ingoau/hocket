import { Albums } from "./Albums";

export function GenreDetail({ name }: { name: string }) {
  return <Albums genre={name} title={name} />;
}
