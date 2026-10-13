/** Presentation only: never convert IDs in IPC payloads, telemetry or stored text. */
export function speakerLabel(value: string | null | undefined): string {
  if (!value) return "待选择";
  const match = /^(AFF|NEG)_?([1-4])$/i.exec(value.trim());
  if (match)
    return `${match[1].toUpperCase() === "AFF" ? "正方" : "反方"}${["一", "二", "三", "四"][Number(match[2]) - 1]}辩`;
  return /^(正方|反方)[一二三四]辩/.test(value) ? value : "待核对辩位";
}

/** System descriptions can contain Rust's Aff1/Neg1 names; IDs stay unchanged. */
export function readableRoleText(text: string): string {
  return text.replace(
    /(^|[^A-Za-z0-9_-])((?:AFF|NEG)_?[1-4])(?=$|[^A-Za-z0-9_-])/gi,
    (_match, prefix: string, code: string) => prefix + speakerLabel(code),
  );
}
