### Memory benchmark: `hobnail-rp:m5` (sha256:14897f701f99), profile app, warm cache

2026-11-19T05:19:35Z; config none (empty); host 4 CPUs, 7.2 GiB, kernel 7.0.0-38-generic, Docker 29.5.2; start -> first /healthz 200: 446 ms

| Phase | Samples | `memory.current` median / max | Working set median / max | `memory.peak` at end | anon / file / kernel / sock median | CPU % of one core | Requests / errors / events | p50 / p99 ms |
|---|---|---|---|---|---|---|---|---|
| startup | 100 | 1.54 / 1.96 | 1.27 / 1.71 | 4.06 | 0.78 / 0.26 / 0.41 / 0.00 | 0.46 | – | – |
| idle | 180 | 1.64 / 2.28 | 1.28 / 2.02 | 4.98 | 0.83 / 0.32 / 0.39 / 0.00 | 0.32 | – | – |
| clients-1 | 120 | 2.29 / 3.07 | 1.73 / 2.45 | 21.44 | 1.09 / 0.60 / 0.45 / 0.00 | 0.66 | 310 / 0 / 37 | 1.5 / 6.6 |
| clients-10 | 120 | 3.23 / 4.49 | 2.46 / 3.61 | 21.95 | 1.38 / 0.75 / 0.52 / 0.00 | 3.98 | 3100 / 0 / 364 | 1.4 / 10.7 |
| clients-25 | 120 | 3.93 / 4.29 | 2.91 / 3.29 | 22.24 | 1.75 / 1.00 / 0.58 / 0.00 | 7.98 | 7750 / 0 / 831 | 3.1 / 16.4 |
| after | 120 | 2.74 / 3.38 | 1.50 / 2.25 | 22.24 | 1.05 / 1.22 / 0.43 / 0.00 | 0.24 | – | – |

Process RSS at phase ends (MiB, from /proc/<pid>/status):

| Phase | RssAnon | RssFile | RssShmem | VmRSS |
|---|---|---|---|---|
| startup | 0.89 | 4.39 | 0.00 | 5.28 |
| idle | 0.88 | 4.45 | 0.00 | 5.34 |
| clients-1 | 1.08 | 5.83 | 0.00 | 6.91 |
| clients-10 | 1.13 | 5.83 | 0.00 | 6.96 |
| clients-25 | 1.14 | 5.83 | 0.00 | 6.96 |
| after | 1.03 | 5.83 | 0.00 | 6.86 |

Endpoints: stream `/api/v1/stream?topics=containers,stats,system`, state `/api/v1/containers`, history `/api/history/system?metric=cpu&range=1h`, assets 89: `/app`, `/assets/Appearance-KEv8dnBm.js`, `/assets/Bookmarks-BRwpcWsu.js`, `/assets/Button-DwrunF04.js`, `/assets/Collection-BchEO3Dq.js`, `/assets/Config-DF2qPR0g.js`, `/assets/Containers-DUvRjNnR.js`, `/assets/Dialog-B3uVPhtz.js`, `/assets/EditDialog-BARi-kyo.js`, `/assets/Editor-e7wlhjy2.js`, `/assets/Field-7RUjToNA.js`, `/assets/General-BS7yNxbg.js`, `/assets/History-CSQqn_iB.js`, `/assets/MenuImpl-GkuM2KrT.js`, `/assets/Notifications-C6FCuCAV.js`, `/assets/Palette-D4ScbimI.css`, `/assets/Palette-yNr_pyGY.js`, `/assets/RechartsUsage-CC9LOCIs.css`, `/assets/RechartsUsage-Cv9abRUt.js`, `/assets/RelativeTime-C4TN0sAR.js`, `/assets/Security-8lIEN2kZ.js`, `/assets/Select-Cai2KDVa.js`, `/assets/SettingsForm-DILsZIp3.js`, `/assets/Shortcuts-DSdpKul5.js`, `/assets/Switch-BUS2Y8uf.js`, `/assets/TabNav-CRIRui1C.js`, `/assets/Tabs-ExaiD3g4.js`, `/assets/Toaster-B_zV1Cvv.js`, `/assets/Users-BuWt-_Z_.js`, `/assets/WidgetContent-Dzg06snL.js`, `/assets/Widgets-DmWyKGaL.js`, `/assets/account-B5ciHFge.js`, `/assets/alerts-Y0bxH_1U.js`, `/assets/announce-BaDTwOl2.js`, `/assets/app-runtime-DLsVIt_d.js`, `/assets/board-Di-mSvph.css`, `/assets/board-J3Sjea6r.js`, `/assets/bookmarks-HIjuKMXL.js`, `/assets/button-dp3fTQDb.js`, `/assets/charts-vendor-BhgI_-Ol.js`, `/assets/charts-vendor-C6Pdu-IP.js`, `/assets/children-CYU4CyM7.js`, `/assets/container-BG7CEc1s.js`, `/assets/container-gV5Acizf.css`, `/assets/containers-C9ZYqLk9.js`, `/assets/containers-CNkSD3_X.js`, `/assets/containers-DIOiKCIT.css`, `/assets/createLucideIcon-CRbusLLJ.js`, `/assets/env-i8CXn8sH.js`, `/assets/events-B2Bcfi6U.js`, `/assets/events-BCX97W3_.js`, `/assets/format-CF5Z1gM7.js`, `/assets/forms-DU4HvGk8.js`, `/assets/history-1HslwQfM.js`, `/assets/history-BRBLfKut.js`, `/assets/history-Dlkw40-6.css`, `/assets/host-BUbc8QtP.css`, `/assets/host-Odp2i3mJ.js`, `/assets/index-0Y5ftNI3.js`, `/assets/keyboard-HHPkxq8T.js`, `/assets/layers-B7tDtED_.js`, `/assets/live-DqPGC3QO.js`, `/assets/logs-C7iCcirE.css`, `/assets/logs-CLb8lUjJ.js`, `/assets/modal-attribute-BP04ZJUB.js`, `/assets/overview-Bn4RnX6U.css`, `/assets/overview-D7rpB5gY.js`, `/assets/parts-GyezLEtB.js`, `/assets/pointer-BBzP2ZJc.js`, `/assets/popup-C1bZqKLS.js`, `/assets/preload-helper-Dp5Bp1yV.js`, `/assets/react-CdlCbvX4.js`, `/assets/read-DszbDfUj.js`, `/assets/rolldown-runtime-CNOWq4Kh.js`, `/assets/search-worker-B5Paj1wr.js`, `/assets/session-7ttDCFor.js`, `/assets/settings-DDwTshO5.js`, `/assets/settings-DktKE8Cv.css`, `/assets/settings-Dq18O-bI.js`, `/assets/shared-Biha3iLl.js`, `/assets/sheet-presentation-DyhvGWRA.js`, `/assets/shortcut-slot-Cs7WM6Rv.js`, `/assets/shortcuts-B26-QLIQ.js`, `/assets/sides-Ce7I_LlF.js`, `/assets/triangle-exclamation-point-Bxrvyj48.js`, `/assets/typeahead-D5O04iSg.js`, `/assets/use-primitive-BWYySHcm.js`, `/assets/web-DupGL4Wj.css`, `/assets/web-fgg0mksI.js`

Raw samples: /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/bench-all/samples.csv; RSS: /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/bench-all/rss.csv; clients: /tmp/claude-1000/-home-dev-projects-rust-homepage/b1d6dc1b-01ae-438b-8462-73d11eda49a6/scratchpad/m5/bench-all/clients/
