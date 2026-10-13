import { visibleWindow } from "./window";

export function TranscriptList({
  items,
  start,
  size,
}: {
  items: { id: string; text: string }[];
  start: number;
  size: number;
}) {
  const range = visibleWindow(items.length, start, size);
  const shown = items.slice(range.from, range.to);
  return (
    <ul aria-label="记录">
      {shown.map((item) => (
        <li key={item.id} id={`t-${item.id}`}>
          {item.text}
        </li>
      ))}
    </ul>
  );
}
