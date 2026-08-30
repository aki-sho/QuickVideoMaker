export type WatermarkScale = "small" | "medium" | "large" | "full";

export type VideoDimensions = {
  width: number;
  height: number;
};

export type WatermarkPointOptions = {
  dimensions: VideoDimensions;
  box: VideoDimensions;
  x: number;
  y: number;
  spacing: number;
  count: number;
};

const scalePercent: Record<WatermarkScale, number> = {
  small: 20,
  medium: 35,
  large: 50,
  full: 100,
};

export function initialWatermarkSpacing(width: number) {
  if (width <= 479) return 24;
  if (width <= 1079) return 48;
  return 96;
}

export function calculateWatermarkBox(dimensions: VideoDimensions, scale: WatermarkScale) {
  const percent = scalePercent[scale];
  return {
    width: Math.max(2, Math.floor(dimensions.width * percent / 100)),
    height: Math.max(2, Math.floor(dimensions.height * percent / 100)),
  };
}

export function calculateWatermarkPoints(options: WatermarkPointOptions) {
  const { dimensions, box, spacing, count } = options;
  const maxX = Math.max(0, dimensions.width - box.width);
  const maxY = Math.max(0, dimensions.height - box.height);
  const spanX = maxX + 1;
  const spanY = maxY + 1;
  const baseX = Math.min(maxX, Math.max(0, options.x));
  const baseY = Math.min(maxY, Math.max(0, options.y));
  const stepX = box.width + spacing;
  const stepY = box.height + spacing;
  return Array.from({ length: count }, (_, index) => ({
    x: (baseX + index * stepX) % spanX,
    y: (baseY + index * stepY) % spanY,
  }));
}