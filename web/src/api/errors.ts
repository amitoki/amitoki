import { labels } from "../labels";
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : labels.requestFailed;
}
