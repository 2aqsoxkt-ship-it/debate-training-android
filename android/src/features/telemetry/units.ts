/** Approximate effective units for Chinese debate text (mirrors Rust text_units). */

type GraphemeClass = "cjk" | "latin" | "digit" | "skip";

export function countEffectiveUnits(text: string): number {
  let units = 0;
  let previous: GraphemeClass | null = null;
  for (const grapheme of iterateGraphemes(text)) {
    const cls = classify(grapheme);
    if (startsUnit(cls, previous)) units += 1;
    previous = cls;
  }
  return units;
}

export function splitEffective(text: string, cap: number): [string, string] {
  if (cap <= 0) return ["", text];
  let units = 0;
  let previous: GraphemeClass | null = null;
  let end = 0;
  for (const { offset, grapheme } of iterateGraphemeIndices(text)) {
    const cls = classify(grapheme);
    const next = startsUnit(cls, previous) ? units + 1 : units;
    if (next > cap) break;
    units = next;
    previous = cls;
    end = offset + grapheme.length;
  }
  return [text.slice(0, end), text.slice(end)];
}

function startsUnit(
  cls: GraphemeClass,
  previous: GraphemeClass | null,
): boolean {
  if (cls === "cjk") return true;
  if (cls === "latin") return previous !== "latin";
  if (cls === "digit") return previous !== "digit";
  return false;
}

function classify(grapheme: string): GraphemeClass {
  if (isEmoji(grapheme)) return "skip";
  const base = [...grapheme].find((ch) => !isCombining(ch));
  if (!base) return "skip";
  if (isCjk(base)) return "cjk";
  if (isLatinLetter(base)) return "latin";
  if (isDigit(base)) return "digit";
  return "skip";
}

type GraphemeSegmenter = {
  segment(input: string): Iterable<{ index: number; segment: string }>;
};

function getSegmenter(): GraphemeSegmenter | null {
  const Ctor = (
    Intl as unknown as {
      Segmenter?: new (
        locales?: string,
        options?: { granularity: string },
      ) => GraphemeSegmenter;
    }
  ).Segmenter;
  if (!Ctor) return null;
  return new Ctor(undefined, { granularity: "grapheme" });
}

function iterateGraphemes(text: string): string[] {
  const segmenter = getSegmenter();
  if (segmenter) {
    return [...segmenter.segment(text)].map((s) => s.segment);
  }
  return [...text];
}

function* iterateGraphemeIndices(
  text: string,
): Generator<{ offset: number; grapheme: string }> {
  const segmenter = getSegmenter();
  if (segmenter) {
    for (const s of segmenter.segment(text)) {
      yield { offset: s.index, grapheme: s.segment };
    }
    return;
  }
  let offset = 0;
  for (const ch of text) {
    yield { offset, grapheme: ch };
    offset += ch.length;
  }
}

function isEmoji(grapheme: string): boolean {
  return [...grapheme].some((ch) => {
    const cp = ch.codePointAt(0)!;
    return (
      (cp >= 0x1f000 && cp <= 0x1faff) ||
      (cp >= 0x2600 && cp <= 0x27bf) ||
      ch === "\uFE0F" ||
      ch === "\u200D"
    );
  });
}

function isCombining(ch: string): boolean {
  const cp = ch.codePointAt(0)!;
  return (cp >= 0x0300 && cp <= 0x036f) || (cp >= 0x20d0 && cp <= 0x20ff);
}

function isCjk(ch: string): boolean {
  const cp = ch.codePointAt(0)!;
  return (
    (cp >= 0x3400 && cp <= 0x4dbf) ||
    (cp >= 0x4e00 && cp <= 0x9fff) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0x3040 && cp <= 0x30ff) ||
    (cp >= 0xac00 && cp <= 0xd7af)
  );
}

function isLatinLetter(ch: string): boolean {
  const cp = ch.codePointAt(0)!;
  return (
    (cp >= 0x41 && cp <= 0x5a) ||
    (cp >= 0x61 && cp <= 0x7a) ||
    (cp >= 0x00c0 && cp <= 0x00d6) ||
    (cp >= 0x00d8 && cp <= 0x00f6) ||
    (cp >= 0x00f8 && cp <= 0x024f)
  );
}

function isDigit(ch: string): boolean {
  const cp = ch.codePointAt(0)!;
  return (cp >= 0x30 && cp <= 0x39) || (cp >= 0xff10 && cp <= 0xff19);
}
