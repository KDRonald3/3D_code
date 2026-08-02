/**
 * Protocol helpers — re-exports W2 `webviewHost` types plus the inspection
 * payload shape used by `inspection.ts`.
 *
 * Canonical message unions live in `webviewHost.ts` (keep aligned with
 * `media/bridge.js` / README.md).
 */

export type {
  HostToWebviewMessage,
  MapWebviewMessage,
  WebviewToHostMessage,
} from "./webviewHost";

/** Normalised selectFunction payload for the inspection canvas. */
export interface SelectFunctionPayload {
  type: "selectFunction";
  functionId: string | null;
  fileId: string | null;
  filePath: string | null;
  /**
   * Free-function display name when the webview sends it. Optional — host
   * derives from `functionId` (`…::name` → `name`) when absent.
   */
  functionName: string | null;
  /** 1-based line of the `fn` keyword when known. */
  line: number | null;
  /** UTF-8 byte offset of the function item start (inclusive). */
  byteStart: number | null;
  /** UTF-8 byte offset one past the function item end. */
  byteEnd: number | null;
  /** Hex SHA-256 from the map File node; empty/null = unverifiable. */
  contentHash: string | null;
}
