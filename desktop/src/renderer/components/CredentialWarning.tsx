// Shown wherever a password is entered or listed when main can only keep it
// in memory (no usable OS keyring): the user must know it won't be remembered.
import { t } from "@shared/strings";
import { useApp } from "../store/app";

export function CredentialWarning() {
  const storage = useApp((s) => s.meta?.credentialStorage);
  if (storage !== "volatile") return null;
  return <div className="credential-warning" role="alert" data-testid="credential-warning">{t("settings.credentialsVolatile")}</div>;
}
