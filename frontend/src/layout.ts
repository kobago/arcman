export type Placed<T> = { item: T; width: number }
export type Row<T> = { height: number; tiles: Placed<T>[] }

export function aspectRatio(width?: number | null, height?: number | null): number {
  if (!width || !height) return 1
  return Math.min(2.5, Math.max(0.5, width / height))
}

export function justify<T>(
  items: T[],
  ratioOf: (item: T) => number,
  containerWidth: number,
  targetHeight: number,
  gap: number,
): Row<T>[] {
  const rows: Row<T>[] = []
  let pending: { item: T; ratio: number }[] = []
  let ratioSum = 0
  const close = (height: number) => {
    rows.push({ height, tiles: pending.map((p) => ({ item: p.item, width: p.ratio * height })) })
    pending = []
    ratioSum = 0
  }
  for (const item of items) {
    const ratio = ratioOf(item)
    pending.push({ item, ratio })
    ratioSum += ratio
    const gaps = gap * (pending.length - 1)
    if (ratioSum * targetHeight + gaps >= containerWidth) {
      close((containerWidth - gaps) / ratioSum)
    }
  }
  if (pending.length) close(targetHeight)
  return rows
}
