/**
 * The only place that talks to Rust.
 *
 * Every command returns a `{ status, data | error }` union; `call` turns it into
 * a resolved value or a thrown {@link ApiError} so React Query sees real errors.
 */
import { commands, events } from "./bindings";
import type {
  BulkPatch_Deserialize,
  CaptureParseDto_Serialize,
  PostingPatch_Deserialize,
  QueryRequest_Deserialize,
  SaveTxnPatch_Deserialize,
  Settings_Serialize,
  TagDto_Deserialize,
  VaultCreateOptions_Deserialize,
} from "./bindings";

export type { Settings_Serialize as Settings };

export class ApiError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

type WireResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: unknown };

function isWireError(error: unknown): error is { code: string; message: string } {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof (error as { code: unknown }).code === "string"
  );
}

async function call<T>(pending: Promise<WireResult<T>>): Promise<T> {
  const result = await pending;
  if (result.status === "ok") return result.data;
  if (isWireError(result.error)) {
    throw new ApiError(result.error.code, result.error.message);
  }
  throw new ApiError("ipc.unknown", String(result.error));
}

export const api = {
  // vault
  vaultCreate: (path: string, opts: VaultCreateOptions_Deserialize | null = null) =>
    call(commands.vaultCreate(path, opts)),
  vaultOpen: (path: string) => call(commands.vaultOpen(path)),
  vaultClose: () => call(commands.vaultClose()),
  vaultRecent: () => call(commands.vaultRecent()),
  vaultInfo: () => call(commands.vaultInfo()),
  reload: () => call(commands.reloadVault()),
  sampleVaultPath: () => call(commands.sampleVaultPath()),
  createSampleVault: () => call(commands.createSampleVault()),

  // reads
  snapshot: () => call(commands.ledgerSnapshot()),
  transactions: (req: QueryRequest_Deserialize) => call(commands.ledgerQuery(req)),
  dashboard: (months: number | null = null) => call(commands.dashboard(months)),
  accounts: () => call(commands.accountsTree()),
  rules: () => call(commands.ruleList()),
  budgets: () => call(commands.budgetList()),
  forecast: (months: number | null = null) => call(commands.forecast(months)),
  fxMissing: () => call(commands.fxMissing()),
  audit: (limit: number) => call(commands.writeAudit(limit)),

  // writes
  captureParse: (text: string) => call(commands.captureParse(text)),
  captureCommit: (text: string) => call(commands.captureCommit(text)),
  txnGet: (id: string) => call(commands.txnGet(id)),
  txnUpdate: (id: string, patch: SaveTxnPatch_Deserialize) =>
    call(commands.txnUpdate(id, patch)),
  txnDelete: (id: string) => call(commands.txnDelete(id)),
  txnBulkUpdate: (ids: string[], patch: BulkPatch_Deserialize) =>
    call(commands.txnBulkUpdate(ids, patch)),
  ruleUpsert: (rawLine: string) => call(commands.ruleUpsert(rawLine)),
  ruleToggle: (id: string) => call(commands.ruleToggle(id)),
  ruleTest: (id: string, n = 20) => call(commands.ruleTest(id, n)),
  ruleRecompute: (ids: string[]) => call(commands.ruleRecompute(ids)),
  accountUpsert: (name: string, kind: string) => call(commands.accountUpsert(name, kind)),
  fxApplyRates: (rows: { date: string; from: string; to: string; rate: string }[]) =>
    call(commands.fxApplyRates(rows)),

  // settings
  settings: () => call(commands.settingsGet()),
  saveSettings: (settings: Settings_Serialize) => call(commands.settingsSet(settings)),
  version: () => call(commands.appVersion()),
  shortcutStatus: () => call(commands.shortcutStatus()),

  // capture window
  showCapture: () => call(commands.captureShow()),
  hideCapture: () => call(commands.captureHide()),
};

export type CapturePreview = CaptureParseDto_Serialize;
export type TagInput = TagDto_Deserialize;
export type PostingPatch = PostingPatch_Deserialize;

export const onLedgerChanged = events.ledgerChanged.listen;
export const onCaptureSaved = events.captureSaved.listen;
export const onParseDiagnostics = events.parseDiagnostics.listen;

export const queryKeys = {
  snapshot: ["snapshot"] as const,
  dashboard: (months: number) => ["dashboard", months] as const,
  transactions: (req: QueryRequest_Deserialize) => ["transactions", req] as const,
  accounts: ["accounts"] as const,
  rules: ["rules"] as const,
  settings: ["settings"] as const,
  fxMissing: ["fx-missing"] as const,
  audit: ["audit"] as const,
  shortcutStatus: ["shortcut-status"] as const,
  ruleTest: (id: string) => ["rule-test", id] as const,
};
