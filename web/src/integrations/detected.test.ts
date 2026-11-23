import assert from "node:assert/strict";
import { test } from "node:test";
import type { DetectedService } from "../api/gen/DetectedService.ts";
import type { Provider } from "../api/gen/Provider.ts";
import { DISMISSED_KEY, detectedSummary, detectionKey, imageName, prefillFor, proposedUrl, readDismissed, viaLabel, visibleDetected, writeDismissed } from "./detected.ts";

const sonarr: DetectedService = {
  provider: "sonarr", container: "sonarr", image: "lscr.io/linuxserver/sonarr:latest", matched: "image", port: 8989,
  candidates: [{ url: "http://192.168.1.50:8989", via: "published" }, { url: "http://sonarr:8989", via: "name" }],
};
const jellyfin: DetectedService = { provider: "jellyfin", container: "media", image: "jellyfin/jellyfin:10.10", matched: "image", port: 8096, candidates: [] };
const providers = [{ id: "sonarr", name: "Sonarr" }, { id: "jellyfin", name: "Jellyfin" }] as Provider[];

test("image names drop registry, tag and digest", () => {
  assert.equal(imageName("lscr.io/linuxserver/sonarr:latest"), "linuxserver/sonarr");
  assert.equal(imageName("pihole/pihole:2024.07.0"), "pihole/pihole");
  assert.equal(imageName("postgres:16-alpine"), "postgres");
  assert.equal(imageName("localhost:5000/team/app@sha256:abc"), "team/app");
  assert.equal(detectedSummary(sonarr), "linuxserver/sonarr, port 8989");
});

test("the first candidate is the proposed URL", () => {
  assert.equal(proposedUrl(sonarr), "http://192.168.1.50:8989");
  assert.equal(viaLabel(sonarr), "via published port");
  assert.equal(proposedUrl(jellyfin), "");
  assert.match(viaLabel(jellyfin), /Enter the URL/);
  assert.deepEqual(prefillFor(sonarr, { name: "Sonarr" }), { provider: "sonarr", url: "http://192.168.1.50:8989", name: "Sonarr" });
});

test("dismissed and unknown services are hidden", () => {
  const unknown = { ...sonarr, provider: "retired" };
  assert.deepEqual(visibleDetected([sonarr, jellyfin, unknown], new Set([detectionKey(jellyfin)]), providers), [sonarr]);
});

test("dismissals survive a reload and bad storage", () => {
  const map = new Map<string, string>();
  const store = { getItem: (k: string) => map.get(k) ?? null, setItem: (k: string, v: string) => void map.set(k, v) };
  writeDismissed(store, new Set(["sonarr:sonarr"]));
  assert.deepEqual([...readDismissed(store)], ["sonarr:sonarr"]);
  map.set(DISMISSED_KEY, "{not json");
  assert.equal(readDismissed(store).size, 0);
  map.set(DISMISSED_KEY, JSON.stringify([1, "a:b"]));
  assert.deepEqual([...readDismissed(store)], ["a:b"]);
  const blocked = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
  assert.equal(readDismissed(blocked).size, 0);
  writeDismissed(blocked, new Set(["x"]));
});
