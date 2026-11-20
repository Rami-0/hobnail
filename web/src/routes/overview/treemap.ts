type Box = [x: number, y: number, width: number, height: number];
const sum = (ns: number[]) => ns.reduce((total, n) => total + n, 0);
const worst = (row: number[], side: number) => {
  const ratio = (sum(row) / side) ** 2;
  return Math.max(Math.max(...row) / ratio, ratio / Math.min(...row));
};

function squarify(weights: number[], areaWidth: number, areaHeight: number): Box[] {
  const factor = areaWidth * areaHeight / sum(weights);
  const areas = weights.map((n) => n * factor);
  let [x, y, width, height] = [0, 0, areaWidth, areaHeight];
  const boxes: Box[] = [];
  for (let i = 0; i < areas.length;) {
    const side = Math.min(width, height);
    let j = i + 1;
    while (j < areas.length && worst(areas.slice(i, j + 1), side) <= worst(areas.slice(i, j), side)) j++;
    const row = areas.slice(i, j);
    const fixed = sum(row) / side;
    let offset = 0;
    for (const area of row) {
      const length = area / fixed;
      boxes.push(width >= height ? [x, y + offset, fixed, length] : [x + offset, y, length, fixed]);
      offset += length;
    }
    if (width >= height) { x += fixed; width -= fixed; }
    else { y += fixed; height -= fixed; }
    i = j;
  }
  return boxes;
}

/**
 * Lay out descending positive weights in a `width` × `height` pixel area.
 * Boxes smaller than `min` (room for a name and a size) fold into a final "others" tile.
 */
export function treemap(weights: number[], width: number, height: number, min: [number, number]) {
  let shown = Math.min(weights.length, 18);
  let boxes: Box[];
  do {
    const tail = sum(weights.slice(shown));
    boxes = squarify(weights.slice(0, shown).concat(tail > 0 ? [tail] : []), width, height);
    if (boxes.every(([, , w, h]) => w >= min[0] && h >= min[1])) break;
  } while (shown-- > 0);
  return { boxes, shown: Math.max(0, shown) };
}
