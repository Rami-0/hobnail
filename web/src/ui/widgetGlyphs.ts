// One icon per generic stat-tile glyph (widgets::Glyph). Providers choose a
// glyph by meaning; nothing here knows which provider asked.
import type { LucideIcon } from "lucide-react";
import {
  ActivityIcon, ArrowDownIcon, ArrowUpIcon, CalendarIcon, CircleCheckIcon, ClockIcon, CpuIcon, DatabaseIcon,
  FilmIcon, GlobeIcon, HardDriveIcon, ImageIcon, MemoryStickIcon, MusicIcon, PlayIcon, ServerIcon, ShieldIcon,
  TriangleAlertIcon, TvIcon, UsersIcon,
} from "lucide-react";
import type { Glyph } from "../api/gen/Glyph.ts";

export const GLYPHS: Record<Glyph, LucideIcon> = {
  activity: ActivityIcon, alert: TriangleAlertIcon, calendar: CalendarIcon, check: CircleCheckIcon, clock: ClockIcon,
  cpu: CpuIcon, database: DatabaseIcon, disk: HardDriveIcon, download: ArrowDownIcon, film: FilmIcon, globe: GlobeIcon,
  image: ImageIcon, memory: MemoryStickIcon, music: MusicIcon, play: PlayIcon, server: ServerIcon, shield: ShieldIcon,
  tv: TvIcon, upload: ArrowUpIcon, users: UsersIcon,
};
