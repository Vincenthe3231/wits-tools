/** Reads safe identifiers from rejected operations and provides a shared fallback. */

// Treat strings, native exceptions, and malformed IPC responses as unknown failures.
export function errorCode<const Code extends string>(
  cause: unknown,
  knownCodes: readonly Code[],
): Code | "unexpected" {
  if (cause && typeof cause === "object" && "code" in cause) {
    return knownCodes.find((code) => code === cause.code) ?? "unexpected";
  }
  return "unexpected";
}
