import { toast } from "sonner";
import i18n from "@/i18n";

type ErrorResult = { status: "error"; error: unknown };

const isErrorResult = (r: unknown): r is ErrorResult =>
  !!r &&
  typeof r === "object" &&
  (r as { status?: unknown }).status === "error";

/**
 * Await a Tauri command. Commands report failure as `{ status: "error" }`
 * instead of throwing; this throws it, so one `catch` handles both.
 */
export async function ok<T>(call: Promise<T>): Promise<T> {
  const r = await call;
  if (isErrorResult(r)) throw new Error(String(r.error));
  return r;
}

export const errorText = (e: unknown): string =>
  e instanceof Error ? e.message : String(e);

/** Toast a failed action with its reason. `key` takes an `{{error}}` value. */
export const toastError = (key: string, e: unknown) => {
  console.error(key, e);
  toast.error(i18n.t(key, { error: errorText(e) }));
};
