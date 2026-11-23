// Vendored brand icons (GET /api/v1/bootstrap `brand_icons`): the server
// serves these without any remote fetch, so they show whatever the D20
// `remote_icons` setting says.
import type { BrandIcon } from "../api/gen/BrandIcon.ts";
import { useBootstrap } from "../live/index.ts";

export function findBrand(icons: readonly BrandIcon[] | undefined, name: string | null | undefined): BrandIcon | undefined {
  return name ? icons?.find((icon) => icon.name === name) : undefined;
}

/** The vendored icon of this name, if the server has one. */
export function useBrand(name: string | null | undefined): BrandIcon | undefined {
  return findBrand(useBootstrap()?.brand_icons, name);
}

/** A brand colour as a CSS custom property value, only when it is a plain hex colour. */
export function brandColor(color: string | null | undefined): string | undefined {
  return color && /^#[0-9a-f]{6}$/i.test(color) ? color : undefined;
}
