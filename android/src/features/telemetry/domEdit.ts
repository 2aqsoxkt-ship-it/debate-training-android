import type { FragmentSource, SourceEvent } from "./types";

/** Minimal edit between observed committed values. Never split a surrogate pair. */
export function observedEdit(
  before: string,
  after: string,
  source: FragmentSource,
): SourceEvent | null {
  if (before === after) return null;
  const left = [...before];
  const right = [...after];
  let head = 0;
  while (
    head < left.length &&
    head < right.length &&
    left[head] === right[head]
  )
    head++;
  let tail = 0;
  while (
    tail < left.length - head &&
    tail < right.length - head &&
    left[left.length - 1 - tail] === right[right.length - 1 - tail]
  )
    tail++;
  return {
    kind: "replace",
    start: left.slice(0, head).join("").length,
    end: left.slice(0, left.length - tail).join("").length,
    text: right.slice(head, right.length - tail).join(""),
    source,
  };
}

export function inputSource(
  inputType: string,
): Exclude<FragmentSource, "overflow"> {
  if (["insertFromPaste", "insertFromDrop"].includes(inputType))
    return "pasted";
  if (
    [
      "insertText",
      "insertCompositionText",
      "insertFromComposition",
      "insertReplacementText",
      "insertLineBreak",
      "insertParagraph",
    ].includes(inputType) ||
    inputType.startsWith("delete")
  )
    return "typed";
  // Undo/redo or an input with no metadata cannot establish the inserted source.
  return "unverified";
}
